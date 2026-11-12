//! Sync engine shared by all clients: keeps the conversation list fresh, long-polls
//! open chats and pushes UI-ready events to the platform layer.
//!
//! The platform only renders [`RoomView`]/[`MessageView`] and calls the async methods;
//! all Talk-specific rules (hidden system messages, edits/deletions arriving as system
//! messages, own-message detection, sorting) live here once.
//!
//! With a data directory everything is cached in SQLite ([`crate::cache`]): the room
//! list is available before the first request and chats open from disk, then only the
//! delta is fetched.
//!
//! Next step (see roadmap): the HPB signaling connection replacing the room-list polling.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::cache::Cache;
use crate::call::{CallManager, CallView, MediaEngine, MediaReport, RecordingNotice, Recordings, RoomInfo};
use crate::outbox::{Outbox, PendingMessage};
use crate::voice::VoiceStore;

use crate::client::TalkClient;
use crate::error::{Error, Result};
use crate::models::{ChatMessage, FileAttachment, Room, RoomType};

/// Room list refresh while no signaling connection exists yet.
const ROOMS_INTERVAL: Duration = Duration::from_secs(10);
/// Incremental refreshes miss deletions and some counter changes; do a full one regularly.
const ROOMS_FULL_INTERVAL: Duration = Duration::from_secs(60);
const POLL_TIMEOUT_S: u32 = 30;
const PAGE_SIZE: u32 = 50;
/// Messages per delta request (the server's limit for `lookIntoFuture`).
const DELTA_PAGE: usize = 100;
/// More new messages than this since the last sync: drop the cached chat instead of
/// fetching them all (the newest page is enough to show the chat).
const DELTA_MAX: usize = 500;

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
pub(crate) const UPDATING_SYSTEM: &[&str] = &[
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
    /// The call changed (state, participants, mute, speaking). `None`: no call any more
    /// (always preceded by one event with state `Ended`).
    CallChanged { call: Option<CallView> },
    /// An incoming call was missed: not answered in time, or it came in while another
    /// call was in progress. For a "missed call" notification.
    CallMissed {
        token: String,
        room_name: String,
        caller_name: String,
        is_group: bool,
    },
    /// Finished call recordings (transcripts, summaries, failures) waiting for the user:
    /// the full current list. See [`Engine::recording_notices`].
    RecordingsChanged { items: Vec<RecordingNotice> },
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

#[derive(Debug, Clone, Default, Serialize)]
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
    /// The chat changed too much since the cached copy: replace what is shown instead of
    /// merging (only from `open_chat`).
    pub reset: bool,
}

#[derive(Default)]
struct State {
    rooms: HashMap<String, Room>,
    modified_before: Option<i64>,
    last_full: Option<Instant>,
    chats: HashMap<String, JoinHandle<()>>,
    rooms_task: Option<JoinHandle<()>>,
    notify_task: Option<JoinHandle<()>>,
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
    pub(crate) cache: Cache,
    pub(crate) voice: Arc<VoiceStore>,
    pub(crate) calls: CallManager,
    pub(crate) recordings: Recordings,
    /// Runtime of `start`, for spawning from synchronous calls.
    pub(crate) runtime: OnceLock<tokio::runtime::Handle>,
    weak: Weak<Self>,
}

impl Engine {
    /// `data_dir`: app-private directory for the cache (conversations, messages, avatars)
    /// and the outbox (unsent messages). Without one, everything lives in memory only.
    /// Loads the cached conversation list, so [`Self::rooms`] answers without network.
    pub fn new(client: TalkClient, sink: Arc<dyn EventSink>, data_dir: Option<PathBuf>) -> Arc<Self> {
        let cache = Cache::open(data_dir.as_deref());
        let me = cache
            .meta("user_id")
            .unwrap_or_else(|| client.account().user.clone());
        let state = State {
            rooms: cache.rooms().into_iter().map(|r| (r.token.clone(), r)).collect(),
            ..State::default()
        };
        Arc::new_cyclic(|weak| Self {
            client,
            sink,
            me: RwLock::new(me),
            me_resolved: AtomicBool::new(false),
            online: AtomicBool::new(true),
            state: Mutex::new(state),
            attachment_folder: RwLock::new(cache.meta("attachment_folder")),
            outbox: Outbox::open(data_dir.as_deref()),
            voice: Arc::new(VoiceStore::open(data_dir.as_deref())),
            cache,
            calls: CallManager::default(),
            recordings: Recordings::default(),
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
        let weak = Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            if let Some(e) = weak.upgrade() {
                e.cache.prune_media();
                e.voice.prune_files();
            }
        });
        // User id and room list in parallel: saves a full round trip on cold start.
        let (user, rooms) = tokio::join!(self.client.current_user(), self.client.rooms(None, false));
        let user = match self.guard(user) {
            Ok(u) => {
                self.set_user(u.id);
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
        let notify = tokio::spawn(Engine::notify_push_loop(Arc::downgrade(self)));
        if let Some(old) = self.state.lock().unwrap().notify_task.replace(notify) {
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
        self.set_user(user.id);
        Ok(())
    }

    fn set_user(&self, id: String) {
        if *self.me.read().unwrap() != id {
            *self.me.write().unwrap() = id.clone();
        }
        self.cache.set_meta("user_id", id);
        self.me_resolved.store(true, Ordering::Relaxed);
    }

    // --- calls (see `crate::call` and docs/calls.md) -----------------------------------

    /// Registers the platform's WebRTC implementation. Call once before any call starts.
    pub fn set_media_engine(&self, media: Arc<dyn MediaEngine>) {
        self.calls.set_media_engine(media);
    }

    /// The call in progress (also an incoming one that is ringing), if any.
    pub fn current_call(&self) -> Option<CallView> {
        self.calls.current()
    }

    /// Calls the conversation (or joins its running call). `silent`: nobody is rung
    /// (automated tests in shared rooms). Fails if another call is in progress.
    /// `call_id`: optional id for the new call (iOS: the CallKit UUID); generated if `None`.
    pub async fn start_call(&self, token: &str, silent: bool, call_id: Option<String>) -> Result<()> {
        if self.calls.busy() {
            return Err(Error::Call {
                code: "busy".into(),
                message: "Es läuft bereits ein Anruf".into(),
            });
        }
        let room = self.room_info(token).await?;
        let rt = self.runtime.get().cloned().unwrap_or_else(tokio::runtime::Handle::current);
        self.calls
            .start(self.client.clone(), self.sink.clone(), self.user_id(), room, silent, call_id, &rt)
    }

    /// Answers the ringing incoming call of `token`.
    pub async fn accept_call(&self, token: &str) -> Result<()> {
        self.calls.accept(token)
    }

    pub(crate) fn state_room(&self, token: &str) -> Option<Room> {
        self.state.lock().unwrap().rooms.get(token).cloned()
    }

    /// Fresh room data for a call (`hasCall` must be current); falls back to the cache.
    async fn room_info(&self, token: &str) -> Result<RoomInfo> {
        let room = match self.client.room(token).await {
            Ok(r) => r,
            Err(e) => match self.state.lock().unwrap().rooms.get(token) {
                Some(r) => r.clone(),
                None => return Err(e),
            },
        };
        Ok(RoomInfo {
            token: room.token.clone(),
            name: room.display_name.clone(),
            is_group: !matches!(room.room_type, RoomType::OneToOne | RoomType::FormerOneToOne),
            has_call: room.has_call,
        })
    }

    /// Stops ringing for the incoming call of `token` on this device.
    pub fn decline_call(&self, token: &str) {
        self.calls.decline(token);
    }

    /// Leaves the current call (or stops dialing). No-op without a call.
    pub fn hang_up(&self) {
        self.calls.hang_up();
    }

    /// Hangs up and waits (at most `timeout`) until the server knows we left: call this
    /// before quitting the app.
    pub async fn hang_up_and_wait(&self, timeout: Duration) {
        self.calls.hang_up_and_wait(timeout).await;
    }

    pub fn set_call_muted(&self, muted: bool) {
        self.calls.set_muted(muted);
    }

    /// Camera on/off in the running call (ignored while ringing; calls start without).
    pub fn set_camera_enabled(&self, on: bool) {
        self.calls.set_camera(on);
    }

    /// The user agreed to being recorded (answer to `CallView::recording_consent_pending`);
    /// the core then joins the call.
    pub fn consent_to_recording(&self) {
        self.calls.consent_to_recording();
    }

    /// Tests (talk-cli): full rejoin of the running call, as after a long outage.
    #[doc(hidden)]
    pub fn debug_force_rejoin(&self) {
        self.calls.force_rejoin();
    }

    /// Feedback from the platform's [`MediaEngine`].
    pub fn media_report(&self, report: MediaReport) {
        self.calls.report(report);
    }

    /// Stops all background work (unsent messages stay stored for the next start).
    /// A running call is hung up (best effort: use [`Self::hang_up_and_wait`] first to make
    /// sure the server knows).
    pub fn shutdown(&self) {
        self.calls.hang_up();
        self.outbox.abort_all();
        let mut st = self.state.lock().unwrap();
        if let Some(t) = st.rooms_task.take() {
            t.abort();
        }
        if let Some(t) = st.notify_task.take() {
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
        if full || !batch.rooms.is_empty() {
            self.cache.put_rooms(batch.rooms.clone(), full);
        }
        let (views, call_started) = {
            let mut st = self.state.lock().unwrap();
            // A call starting in a conversation may ring for us (safety net for notify_push).
            let call_started = batch
                .rooms
                .iter()
                .any(|r| r.has_call && !st.rooms.get(&r.token).is_some_and(|old| old.has_call));
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
            (self.sorted_views(&st.rooms), call_started)
        };
        if call_started {
            self.spawn_incoming_check();
        }
        self.sink.emit(Event::RoomsChanged { rooms: views });
    }

    /// Newest messages from the cache (empty without one), for the first frame of a chat.
    /// Call [`Self::open_chat`] afterwards for the server state.
    pub fn cached_chat(&self, token: &str) -> ChatPage {
        let Some(range) = self.cache.range(token) else {
            return ChatPage::default();
        };
        let (messages, more) = self.cache.messages(token, None, PAGE_SIZE);
        self.cached_page(messages, more || range.has_more, range.last_common_read)
    }

    /// Brings a chat up to date and starts live updates for it. Returns its newest page.
    /// With a cache only the messages since the last sync are fetched (edits, deletions
    /// and reactions arrive among them as system messages).
    ///
    /// Offline, the error is returned but live updates still start from the cached state,
    /// so the chat catches up once the server is reachable.
    pub async fn open_chat(self: &Arc<Self>, token: &str) -> Result<ChatPage> {
        let synced = if self.cache.enabled() {
            self.sync_chat(token).await
        } else {
            self.guard(self.client.history(token, None, PAGE_SIZE).await).map(|batch| {
                // `X-Chat-Last-Given` points at the *oldest* message for history requests.
                let newest = batch.messages.iter().map(|m| m.id).max().unwrap_or(0);
                (self.page(batch.messages, batch.last_common_read), newest)
            })
        };
        match synced {
            Ok((page, newest)) => {
                self.start_poll(token, newest);
                Ok(page)
            }
            Err(Error::Unauthorized) => Err(Error::Unauthorized),
            Err(e) => {
                if let Some(r) = self.cache.range(token) {
                    self.start_poll(token, r.newest_id);
                }
                Err(e)
            }
        }
    }

    fn start_poll(self: &Arc<Self>, token: &str, newest: i64) {
        let weak = Arc::downgrade(self);
        let tok = token.to_string();
        let task = tokio::spawn(async move { poll_loop(weak, tok, newest).await });
        if let Some(old) = self.state.lock().unwrap().chats.insert(token.to_string(), task) {
            old.abort();
        }
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

    /// Fetches the delta since the cached state (or the newest page) into the cache and
    /// returns the newest page from it plus the newest message id.
    async fn sync_chat(&self, token: &str) -> Result<(ChatPage, i64)> {
        let cached = self.cache.range(token);
        let delta = match cached {
            Some(r) => self.fetch_delta(token, r.newest_id).await?,
            None => None,
        };
        let fresh = match delta {
            Some((messages, last_common_read)) => {
                let updates = parent_updates(&messages);
                self.cache.append(token, messages, updates, last_common_read);
                None
            }
            None => {
                let batch = self.guard(self.client.history(token, None, PAGE_SIZE).await)?;
                let has_more = batch.messages.len() as u32 >= PAGE_SIZE;
                self.cache
                    .replace_chat(token, batch.messages.clone(), has_more, batch.last_common_read);
                Some(batch)
            }
        };
        self.cache.flush().await;
        let range = self.cache.range(token);
        let reset = cached.is_some() && fresh.is_some();
        let mut page = match (range, fresh) {
            (Some(r), _) => {
                let (messages, more) = self.cache.messages(token, None, PAGE_SIZE);
                self.cached_page(messages, more || r.has_more, r.last_common_read)
            }
            // Empty chat, or the cache could not store it.
            (None, Some(batch)) => self.page(batch.messages, batch.last_common_read),
            (None, None) => ChatPage::default(),
        };
        page.reset = reset;
        let newest = range.map_or(0, |r| r.newest_id).max(page.messages.iter().map(|m| m.id).max().unwrap_or(0));
        Ok((page, newest))
    }

    /// Messages after `last`, or `None` when the cached copy is not worth updating (too
    /// many new messages, or the history was cleared).
    async fn fetch_delta(&self, token: &str, mut last: i64) -> Result<Option<(Vec<ChatMessage>, Option<i64>)>> {
        let mut out = Vec::new();
        let mut last_common_read = None;
        loop {
            let batch = self.guard(self.client.poll_messages(token, last, 0).await)?;
            last_common_read = batch.last_common_read.or(last_common_read);
            let n = batch.messages.len();
            let before = last;
            if let Some(id) = batch.last_given.or(batch.messages.last().map(|m| m.id)) {
                last = last.max(id);
            }
            out.extend(batch.messages);
            if out.len() > DELTA_MAX || out.iter().any(|m| m.system_message == "history_cleared") {
                return Ok(None);
            }
            if n < DELTA_PAGE || last == before {
                return Ok(Some((out, last_common_read)));
            }
        }
    }

    /// Messages older than `before_id`: from the cache when it has them, else from the
    /// server (then they extend the cached range).
    pub async fn load_older(&self, token: &str, before_id: i64) -> Result<ChatPage> {
        if let Some(r) = self.cache.range(token).filter(|r| before_id > r.oldest_id) {
            let (messages, more) = self.cache.messages(token, Some(before_id), PAGE_SIZE);
            if !messages.is_empty() {
                return Ok(self.cached_page(messages, more || r.has_more, None));
            }
        }
        let batch = self.guard(self.client.history(token, Some(before_id), PAGE_SIZE).await)?;
        let has_more = batch.messages.len() as u32 >= PAGE_SIZE;
        self.cache.prepend(token, before_id, batch.messages.clone(), has_more);
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
            if let Some(room) = st.rooms.get_mut(token) {
                room.last_activity = room.last_activity.max(msg.timestamp);
                room.last_message = Some(msg);
                room.unread_messages = 0;
                room.unread_mention = false;
                self.cache.put_rooms(vec![room.clone()], false);
            }
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
                    self.cache.put_rooms(vec![r.clone()], false);
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
        self.cache.clear();
        self.client.revoke_app_password().await
    }

    /// Conversation avatar (bytes, content type); cached on disk per avatar version.
    pub async fn avatar(&self, token: &str, dark: bool) -> Result<(Vec<u8>, String)> {
        let version = self
            .state
            .lock()
            .unwrap()
            .rooms
            .get(token)
            .map(|r| r.avatar_version.clone())
            .unwrap_or_default();
        let key = format!("avatar/{token}/{version}/{dark}");
        if !version.is_empty() {
            if let Some(hit) = self.cache.media_get(&key).await {
                return Ok(hit);
            }
        }
        let (data, mime) = self.client.room_avatar(token, dark).await?;
        if !version.is_empty() {
            self.cache.media_put(&key, &data, &mime).await;
        }
        Ok((data, mime))
    }

    /// Avatar of a user (call tiles), cached like other media.
    pub async fn user_avatar(&self, user_id: &str, dark: bool) -> Result<(Vec<u8>, String)> {
        let key = format!("avatar-user/{user_id}/{dark}");
        if let Some(hit) = self.cache.media_get(&key).await {
            return Ok(hit);
        }
        let (data, mime) = self.client.user_avatar(user_id, dark).await?;
        self.cache.media_put(&key, &data, &mime).await;
        Ok((data, mime))
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
                .share_to_room(token, &remote, reference_id, caption, reply_to, false)
                .await,
        )
    }

    /// Image preview bytes (JPEG/PNG) of a shared file, fitting into `width`×`height`.
    /// Cached on disk: shared files don't change.
    pub async fn file_preview(&self, file_id: &str, width: u32, height: u32) -> Result<Vec<u8>> {
        let key = format!("preview/{file_id}/{width}x{height}");
        if let Some((data, _)) = self.cache.media_get(&key).await {
            return Ok(data);
        }
        let data = self.client.file_preview(file_id, width, height).await?;
        self.cache.media_put(&key, &data, "").await;
        Ok(data)
    }

    pub(crate) async fn attachment_folder(&self) -> Result<String> {
        if let Some(f) = self.attachment_folder.read().unwrap().clone() {
            return Ok(f);
        }
        let f = self.guard(self.client.attachment_folder().await)?;
        *self.attachment_folder.write().unwrap() = Some(f.clone());
        self.cache.set_meta("attachment_folder", f.clone());
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
            reset: false,
        }
    }

    /// A page of cached messages (already known to be in the cache's range).
    fn cached_page(&self, messages: Vec<ChatMessage>, has_more: bool, last_common_read: Option<i64>) -> ChatPage {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        ChatPage {
            oldest_id: messages.iter().map(|m| m.id).min(),
            messages: messages
                .iter()
                .filter(|m| is_visible(m) && !(m.expiration_timestamp > 0 && m.expiration_timestamp < now))
                .map(|m| self.view(m))
                .collect(),
            has_more,
            last_common_read,
            reset: false,
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
        if messages.iter().any(|m| m.system_message == "history_cleared") {
            // Forget the cached copy; the next open loads the remaining history.
            self.cache.replace_chat(token, Vec::new(), false, None);
        } else if !messages.is_empty() {
            self.cache.append(token, messages.clone(), parent_updates(&messages), None);
        }
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
                if let Some(room) = st.rooms.get_mut(token) {
                    room.last_activity = room.last_activity.max(m.timestamp);
                    room.last_message = Some(m);
                    self.cache.put_rooms(vec![room.clone()], false);
                }
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

/// New versions of earlier messages carried by edit/deletion/reaction system messages.
fn parent_updates(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter(|m| UPDATING_SYSTEM.contains(&m.system_message.as_str()))
        .filter_map(|m| m.parent.as_deref().cloned())
        .collect()
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
                        e.cache.set_read_status(&token, r);
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
