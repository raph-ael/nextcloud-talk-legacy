//! Sync engine shared by all clients: keeps the conversation list fresh, long-polls
//! open chats and pushes UI-ready events to the platform layer.
//!
//! The platform only renders [`RoomView`]/[`MessageView`] and calls the async methods;
//! all Talk-specific rules (hidden system messages, edits/deletions arriving as system
//! messages, own-message detection, sorting) live here once.
//!
//! Next steps (see roadmap): SQLite cache so the UI starts from disk, and the HPB
//! signaling connection replacing the room-list polling interval.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::outbox::{Outbox, PendingMessage};

use crate::client::TalkClient;
use crate::error::{Error, Result};
use crate::models::{ChatMessage, FileAttachment, Room};

/// Room list refresh while no signaling connection exists yet.
const ROOMS_INTERVAL: Duration = Duration::from_secs(10);
/// Incremental refreshes miss deletions and some counter changes; do a full one regularly.
const ROOMS_FULL_INTERVAL: Duration = Duration::from_secs(60);
const POLL_TIMEOUT_S: u32 = 30;
const PAGE_SIZE: u32 = 50;

/// System messages that only modify other messages or are pure noise in a chat view.
const HIDDEN_SYSTEM: &[&str] = &[
    "reaction",
    "reaction_deleted",
    "reaction_revoked",
    "message_deleted",
    "message_edited",
    "history_cleared",
    "poll_voted",
    "call_joined",
    "call_left",
    "thread_created",
    "thread_renamed",
];

/// System messages whose `parent` is the updated version of an existing message.
const UPDATING_SYSTEM: &[&str] = &[
    "reaction",
    "reaction_deleted",
    "reaction_revoked",
    "message_deleted",
    "message_edited",
];

pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: Event);
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    /// Full, sorted conversation list (favorites first, then by activity).
    RoomsChanged { rooms: Vec<RoomView> },
    /// New messages in an open chat, ascending by id.
    MessagesAdded {
        token: String,
        messages: Vec<MessageView>,
    },
    /// Existing messages changed (edited, deleted, reactions).
    MessagesUpdated {
        token: String,
        messages: Vec<MessageView>,
    },
    /// Highest message id everybody has read (for read receipts).
    ReadStatus { token: String, last_common_read: i64 },
    Connection { online: bool },
    /// App password was revoked: the platform must sign out.
    AuthFailed,
    /// The pending (not yet delivered or failed) messages of a conversation changed.
    /// Emitted for every conversation, open or not.
    OutboxChanged {
        token: String,
        items: Vec<PendingMessage>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomView {
    pub token: String,
    pub name: String,
    pub description: String,
    /// See [`crate::models::RoomType`]: 1 one-to-one, 2 group, 3 public, 4 changelog, 6 note to self.
    pub room_type: u8,
    pub unread_messages: i64,
    pub unread_mention: bool,
    pub last_activity: i64,
    pub last_read_message: i64,
    pub last_common_read_message: i64,
    pub last_message: Option<MessagePreview>,
    pub is_favorite: bool,
    pub is_archived: bool,
    pub read_only: bool,
    pub has_call: bool,
    /// 0 default, 1 always, 2 mentions only, 3 never.
    pub notification_level: u8,
    /// Changes whenever the avatar changes: use it as cache key.
    pub avatar_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePreview {
    pub author: String,
    /// One-line text; for file messages only the caption (may be empty).
    pub text: String,
    pub is_own: bool,
    pub is_system: bool,
    pub attachment: Option<FileAttachment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: i64,
    pub token: String,
    pub actor_type: String,
    pub actor_id: String,
    pub author: String,
    pub timestamp: i64,
    /// Message text; for file messages the caption (empty without one).
    pub text: String,
    /// `text`, `system` or `deleted`.
    pub kind: &'static str,
    pub system_message: String,
    pub reference_id: String,
    pub is_own: bool,
    pub edited: bool,
    pub markdown: bool,
    pub reply_to: Option<ReplyPreview>,
    pub reactions: BTreeMap<String, i64>,
    pub reactions_self: Vec<String>,
    pub attachment: Option<FileAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyPreview {
    pub id: i64,
    pub author: String,
    pub text: String,
    pub is_own: bool,
    pub attachment: Option<FileAttachment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPage {
    /// Ascending by id.
    pub messages: Vec<MessageView>,
    /// `false` once the beginning of the conversation is reached.
    pub has_more: bool,
    /// Pass to `load_older`; may be older than the first visible message
    /// (hidden system messages count too).
    pub oldest_id: Option<i64>,
    pub last_common_read: Option<i64>,
}

#[derive(Default)]
struct State {
    rooms: HashMap<String, Room>,
    modified_before: Option<i64>,
    last_full: Option<Instant>,
    chats: HashMap<String, JoinHandle<()>>,
    rooms_task: Option<JoinHandle<()>>,
}

pub struct Engine {
    pub(crate) client: TalkClient,
    pub(crate) sink: Arc<dyn EventSink>,
    me: RwLock<String>,
    /// `me` came from the server (the login name may differ from the user id).
    me_resolved: AtomicBool,
    online: AtomicBool,
    state: Mutex<State>,
    attachment_folder: RwLock<Option<String>>,
    pub(crate) outbox: Outbox,
    /// Runtime of `start`, for spawning from synchronous calls.
    pub(crate) runtime: OnceLock<tokio::runtime::Handle>,
    weak: Weak<Self>,
}

impl Engine {
    /// `data_dir`: where the outbox keeps unsent messages across restarts (an app-private
    /// directory). Without one, unsent messages live in memory only.
    pub fn new(client: TalkClient, sink: Arc<dyn EventSink>, data_dir: Option<PathBuf>) -> Arc<Self> {
        let me = client.account().user.clone();
        Arc::new_cyclic(|weak| Self {
            client,
            sink,
            me: RwLock::new(me),
            me_resolved: AtomicBool::new(false),
            online: AtomicBool::new(true),
            state: Mutex::new(State::default()),
            attachment_folder: RwLock::new(None),
            outbox: Outbox::open(data_dir.as_deref()),
            runtime: OnceLock::new(),
            weak: weak.clone(),
        })
    }

    pub(crate) fn weak_self(&self) -> Weak<Self> {
        self.weak.clone()
    }

    pub fn client(&self) -> &TalkClient {
        &self.client
    }

    /// Resolves the user id, loads the conversation list and starts background refreshes.
    /// Must be called inside a tokio runtime.
    /// Also resumes unsent messages of an earlier run. When offline, background sync and
    /// the outbox start anyway and catch up once the server is reachable.
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        let _ = self.runtime.set(tokio::runtime::Handle::current());
        self.spawn_outbox();
        // User id and room list in parallel: saves a full round trip on cold start.
        let (user, rooms) = tokio::join!(self.client.current_user(), self.client.rooms(None, false));
        let user = match self.guard(user) {
            Ok(u) => {
                *self.me.write().unwrap() = u.id;
                self.me_resolved.store(true, Ordering::Relaxed);
                Ok(())
            }
            Err(Error::Unauthorized) => return Err(Error::Unauthorized),
            Err(e) => Err(e),
        };
        let first = self.guard(rooms).map(|batch| self.apply_rooms(batch, true));

        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(ROOMS_INTERVAL).await;
                let Some(engine) = weak.upgrade() else { break };
                if engine.ensure_user().await.is_err() {
                    continue;
                }
                let full = engine
                    .state
                    .lock()
                    .unwrap()
                    .last_full
                    .is_none_or(|t| t.elapsed() >= ROOMS_FULL_INTERVAL);
                if let Err(Error::Unauthorized) = engine.refresh_rooms(full).await {
                    break;
                }
            }
        });
        if let Some(old) = self.state.lock().unwrap().rooms_task.replace(task) {
            old.abort();
        }
        user.and(first)
    }

    /// Resolves the real user id once (needed for WebDAV paths and own-message detection).
    pub(crate) async fn ensure_user(&self) -> Result<()> {
        if self.me_resolved.load(Ordering::Relaxed) {
            return Ok(());
        }
        let user = self.guard(self.client.current_user().await)?;
        *self.me.write().unwrap() = user.id;
        self.me_resolved.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Stops all background work (unsent messages stay stored for the next start).
    pub fn shutdown(&self) {
        self.outbox.abort_all();
        let mut st = self.state.lock().unwrap();
        if let Some(t) = st.rooms_task.take() {
            t.abort();
        }
        for (_, t) in st.chats.drain() {
            t.abort();
        }
    }

    pub fn user_id(&self) -> String {
        self.me.read().unwrap().clone()
    }

    pub fn rooms(&self) -> Vec<RoomView> {
        let st = self.state.lock().unwrap();
        self.sorted_views(&st.rooms)
    }

    pub async fn refresh_rooms(&self, full: bool) -> Result<()> {
        let since = if full {
            None
        } else {
            self.state.lock().unwrap().modified_before
        };
        let batch = self.guard(self.client.rooms(since, true).await)?;
        self.apply_rooms(batch, since.is_none());
        Ok(())
    }

    fn apply_rooms(&self, batch: crate::client::RoomsBatch, full: bool) {
        let views = {
            let mut st = self.state.lock().unwrap();
            if full {
                st.rooms = batch
                    .rooms
                    .into_iter()
                    .map(|r| (r.token.clone(), r))
                    .collect();
                st.last_full = Some(Instant::now());
            } else if batch.rooms.is_empty() {
                return;
            } else {
                for r in batch.rooms {
                    st.rooms.insert(r.token.clone(), r);
                }
            }
            if batch.modified_before.is_some() {
                st.modified_before = batch.modified_before;
            }
            self.sorted_views(&st.rooms)
        };
        self.sink.emit(Event::RoomsChanged { rooms: views });
    }

    /// Loads the newest page of a chat and starts live updates for it.
    pub async fn open_chat(self: &Arc<Self>, token: &str) -> Result<ChatPage> {
        let batch = self.guard(self.client.history(token, None, PAGE_SIZE).await)?;
        // `X-Chat-Last-Given` points at the *oldest* message for history requests.
        let newest = batch.messages.iter().map(|m| m.id).max().unwrap_or(0);
        let page = self.page(batch.messages, batch.last_common_read);

        let weak = Arc::downgrade(self);
        let tok = token.to_string();
        let task = tokio::spawn(async move { poll_loop(weak, tok, newest).await });
        if let Some(old) = self.state.lock().unwrap().chats.insert(token.to_string(), task) {
            old.abort();
        }
        Ok(page)
    }

    pub fn close_chat(&self, token: &str) {
        if let Some(t) = self.state.lock().unwrap().chats.remove(token) {
            t.abort();
        }
    }

    /// Whether the chat is being live-polled (its echoes will arrive).
    pub(crate) fn chat_open(&self, token: &str) -> bool {
        self.state.lock().unwrap().chats.contains_key(token)
    }

    /// Messages older than `before_id`.
    pub async fn load_older(&self, token: &str, before_id: i64) -> Result<ChatPage> {
        let batch = self.guard(self.client.history(token, Some(before_id), PAGE_SIZE).await)?;
        Ok(self.page(batch.messages, batch.last_common_read))
    }

    /// Sends a text message. The live poll delivers the same message again; match it
    /// via `reference_id` (see [`crate::new_reference_id`]).
    pub async fn send(
        &self,
        token: &str,
        text: &str,
        reference_id: &str,
        reply_to: Option<i64>,
    ) -> Result<MessageView> {
        let msg = self.guard(
            self.client
                .send_message(token, text, reference_id, reply_to)
                .await,
        )?;
        let view = self.view(&msg);
        self.update_room_after_send(token, msg);
        Ok(view)
    }

    fn update_room_after_send(&self, token: &str, msg: ChatMessage) {
        let rooms = {
            let mut st = self.state.lock().unwrap();
            st.rooms.get_mut(token).map(|room| {
                room.last_activity = room.last_activity.max(msg.timestamp);
                room.last_message = Some(msg);
                room.unread_messages = 0;
                room.unread_mention = false;
            });
            self.sorted_views(&st.rooms)
        };
        self.sink.emit(Event::RoomsChanged { rooms });
    }

    /// A message we sent from the outbox: show it in the chat and the room list.
    pub(crate) fn note_own_message(&self, token: &str, msg: ChatMessage) {
        self.sink.emit(Event::MessagesAdded {
            token: token.to_string(),
            messages: vec![self.view(&msg)],
        });
        self.update_room_after_send(token, msg);
    }

    pub async fn mark_read(&self, token: &str, message_id: i64) -> Result<()> {
        let changed = {
            let mut st = self.state.lock().unwrap();
            match st.rooms.get_mut(token) {
                Some(r) if r.unread_messages > 0 || r.last_read_message < message_id => {
                    r.unread_messages = 0;
                    r.unread_mention = false;
                    r.last_read_message = r.last_read_message.max(message_id);
                    Some(self.sorted_views(&st.rooms))
                }
                _ => None,
            }
        };
        let Some(rooms) = changed else { return Ok(()) };
        self.sink.emit(Event::RoomsChanged { rooms });
        self.guard(self.client.set_read_marker(token, message_id).await)
    }

    /// Signs out on the server; call `shutdown` afterwards.
    pub async fn logout(&self) -> Result<()> {
        self.shutdown();
        self.outbox.clear();
        self.client.revoke_app_password().await
    }

    pub async fn avatar(&self, token: &str, dark: bool) -> Result<(Vec<u8>, String)> {
        self.client.room_avatar(token, dark).await
    }

    /// Uploads a local file into the user's Talk attachment folder and posts it to the
    /// conversation. The file message arrives through the live poll; match it via
    /// `reference_id` like a text message.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_file(
        &self,
        token: &str,
        local_path: &str,
        name: &str,
        mime: &str,
        caption: Option<&str>,
        reply_to: Option<i64>,
        reference_id: &str,
    ) -> Result<()> {
        let folder = self.attachment_folder().await?;
        let user = self.user_id();
        let remote = self.guard(
            self.client
                .upload(&user, local_path, &folder, name, mime)
                .await,
        )?;
        self.guard(
            self.client
                .share_to_room(token, &remote, reference_id, caption, reply_to)
                .await,
        )
    }

    /// Image preview bytes (JPEG/PNG) of a shared file, fitting into `width`×`height`.
    pub async fn file_preview(&self, file_id: &str, width: u32, height: u32) -> Result<Vec<u8>> {
        self.client.file_preview(file_id, width, height).await
    }

    pub(crate) async fn attachment_folder(&self) -> Result<String> {
        if let Some(f) = self.attachment_folder.read().unwrap().clone() {
            return Ok(f);
        }
        let f = self.guard(self.client.attachment_folder().await)?;
        *self.attachment_folder.write().unwrap() = Some(f.clone());
        Ok(f)
    }

    // --- internals ---------------------------------------------------------------

    /// Tracks connectivity and auth state for every server call.
    pub(crate) fn guard<T>(&self, r: Result<T>) -> Result<T> {
        match &r {
            Ok(_) => self.set_online(true),
            Err(Error::Unauthorized) => self.sink.emit(Event::AuthFailed),
            Err(Error::Network(_)) => self.set_online(false),
            Err(_) => {}
        }
        r
    }

    pub(crate) fn is_online(&self) -> bool {
        self.online.load(Ordering::Relaxed)
    }

    fn set_online(&self, online: bool) {
        if self.online.swap(online, Ordering::Relaxed) != online {
            self.sink.emit(Event::Connection { online });
            if online {
                self.outbox.wake();
            }
        }
    }

    fn is_own(&self, actor_type: &str, actor_id: &str) -> bool {
        actor_type == "users" && *self.me.read().unwrap() == actor_id
    }

    fn page(&self, messages: Vec<ChatMessage>, last_common_read: Option<i64>) -> ChatPage {
        let has_more = messages.len() as u32 >= PAGE_SIZE;
        let oldest_id = messages.iter().map(|m| m.id).min();
        ChatPage {
            oldest_id,
            messages: messages
                .iter()
                .filter(|m| is_visible(m))
                .map(|m| self.view(m))
                .collect(),
            has_more,
            last_common_read,
        }
    }

    pub fn view(&self, m: &ChatMessage) -> MessageView {
        let deleted = m.message_type == "comment_deleted";
        MessageView {
            id: m.id,
            token: m.token.clone(),
            actor_type: m.actor_type.clone(),
            actor_id: m.actor_id.clone(),
            author: m.actor_display_name.clone(),
            timestamp: m.timestamp,
            text: if deleted { String::new() } else { m.caption_text() },
            kind: if deleted {
                "deleted"
            } else if m.is_system() {
                "system"
            } else {
                "text"
            },
            system_message: m.system_message.clone(),
            reference_id: m.reference_id.clone(),
            is_own: self.is_own(&m.actor_type, &m.actor_id),
            edited: m.last_edit_timestamp.is_some_and(|t| t > 0),
            markdown: m.markdown,
            reply_to: m.parent.as_deref().map(|p| ReplyPreview {
                id: p.id,
                author: p.actor_display_name.clone(),
                text: if p.message_type == "comment_deleted" {
                    String::new()
                } else {
                    preview_text(&p.caption_text())
                },
                is_own: self.is_own(&p.actor_type, &p.actor_id),
                attachment: p.attachment(),
            }),
            reactions: m.reactions.clone(),
            reactions_self: m.reactions_self.clone(),
            attachment: if deleted { None } else { m.attachment() },
        }
    }

    fn sorted_views(&self, rooms: &HashMap<String, Room>) -> Vec<RoomView> {
        let mut list: Vec<&Room> = rooms.values().collect();
        list.sort_by(|a, b| {
            b.is_favorite
                .cmp(&a.is_favorite)
                .then(b.last_activity.cmp(&a.last_activity))
        });
        list.into_iter().map(|r| self.room_view(r)).collect()
    }

    fn room_view(&self, r: &Room) -> RoomView {
        RoomView {
            token: r.token.clone(),
            name: r.display_name.clone(),
            description: r.description.clone(),
            room_type: r.room_type.into(),
            unread_messages: r.unread_messages,
            unread_mention: r.unread_mention,
            last_activity: r.last_activity,
            last_read_message: r.last_read_message,
            last_common_read_message: r.last_common_read_message,
            last_message: r.last_message.as_ref().map(|m| MessagePreview {
                author: m.actor_display_name.clone(),
                text: if m.message_type == "comment_deleted" {
                    String::new()
                } else {
                    preview_text(&m.caption_text())
                },
                is_own: self.is_own(&m.actor_type, &m.actor_id),
                is_system: m.is_system(),
                attachment: m.attachment(),
            }),
            is_favorite: r.is_favorite,
            is_archived: r.is_archived,
            read_only: r.read_only != 0,
            has_call: r.has_call,
            notification_level: r.notification_level,
            avatar_version: r.avatar_version.clone(),
        }
    }

    /// Applies a poll result: emits new and updated messages, refreshes the room preview.
    fn apply_live(&self, token: &str, messages: Vec<ChatMessage>) {
        let mut added = Vec::new();
        let mut updated = Vec::new();
        let mut latest: Option<ChatMessage> = None;
        for m in messages {
            if UPDATING_SYSTEM.contains(&m.system_message.as_str()) {
                if let Some(p) = m.parent.as_deref() {
                    updated.push(self.view(p));
                }
            }
            if is_visible(&m) {
                added.push(self.view(&m));
                latest = Some(m);
            }
        }
        if !updated.is_empty() {
            self.sink.emit(Event::MessagesUpdated {
                token: token.to_string(),
                messages: updated,
            });
        }
        if added.is_empty() {
            return;
        }
        let refs: Vec<String> = added.iter().map(|m| m.reference_id.clone()).collect();
        self.sink.emit(Event::MessagesAdded {
            token: token.to_string(),
            messages: added,
        });
        // After the message itself, so the pending bubble is replaced without a gap.
        self.outbox_echo(token, &refs.iter().map(String::as_str).collect::<Vec<_>>());
        if let Some(m) = latest {
            let rooms = {
                let mut st = self.state.lock().unwrap();
                st.rooms.get_mut(token).map(|room| {
                    room.last_activity = room.last_activity.max(m.timestamp);
                    room.last_message = Some(m);
                });
                self.sorted_views(&st.rooms)
            };
            self.sink.emit(Event::RoomsChanged { rooms });
        }
    }
}

/// One-line preview: drops Markdown syntax and joins lines.
pub fn preview_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(200));
    for line in text.lines() {
        let line = line
            .trim()
            .trim_start_matches(['#', '>'])
            .trim_start_matches("- ")
            .trim_start_matches("* ")
            .trim();
        if line.is_empty() || line.chars().all(|c| matches!(c, '-' | '*' | '_' | '`')) {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&line.replace("**", "").replace("__", "").replace('`', ""));
        if out.len() > 200 {
            break;
        }
    }
    out
}

fn is_visible(m: &ChatMessage) -> bool {
    !HIDDEN_SYSTEM.contains(&m.system_message.as_str())
}

async fn poll_loop(engine: std::sync::Weak<Engine>, token: String, mut last: i64) {
    let mut backoff = Duration::from_secs(1);
    let mut last_read: Option<i64> = None;
    loop {
        let Some(e) = engine.upgrade() else { return };
        let client = e.client.clone();
        drop(e); // don't keep the engine alive across the long poll
        let result = client.poll_messages(&token, last, POLL_TIMEOUT_S).await;
        let Some(e) = engine.upgrade() else { return };
        match e.guard(result) {
            Ok(batch) => {
                backoff = Duration::from_secs(1);
                if let Some(id) = batch.last_given.or(batch.messages.last().map(|m| m.id)) {
                    last = last.max(id);
                }
                if let Some(r) = batch.last_common_read {
                    if last_read != Some(r) {
                        last_read = Some(r);
                        e.sink.emit(Event::ReadStatus {
                            token: token.clone(),
                            last_common_read: r,
                        });
                    }
                }
                e.apply_live(&token, batch.messages);
            }
            Err(Error::Unauthorized) => return,
            Err(err) => {
                tracing::warn!(%token, "chat poll failed: {err}");
                drop(e);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_strips_markdown() {
        assert_eq!(
            preview_text("## ⚙️ Einstellungen\n\n- **fett** und `code`\n---\nEnde"),
            "⚙️ Einstellungen fett und code Ende"
        );
    }

    #[test]
    fn event_json_shape() {
        let ev = Event::ReadStatus {
            token: "abc".into(),
            last_common_read: 5,
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["type"], "readStatus");
        assert_eq!(json["lastCommonRead"], 5);
    }
}
