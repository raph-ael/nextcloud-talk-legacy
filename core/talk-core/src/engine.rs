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
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::cache::Cache;
use crate::call::{CallManager, CallView, MediaEngine, MediaReport, RecordingNotice, Recordings, RoomInfo};
use crate::mentions::{EditDraft, MentionPick, MentionView};
use crate::outbox::{Outbox, PendingMessage};
use crate::search::Index;
use crate::voice::VoiceStore;

use crate::client::TalkClient;
use crate::connection::ConnectionState;
use crate::error::{Error, Result};
use crate::models::{ChatMessage, FileAttachment, GeoLocation, Reactions, Room, RoomType};
use crate::notices::{MessageNotice, Notices};

/// Room list refresh while no signaling connection exists yet.
const ROOMS_INTERVAL: Duration = Duration::from_secs(10);
/// Incremental refreshes miss deletions and some counter changes; do a full one regularly.
const ROOMS_FULL_INTERVAL: Duration = Duration::from_secs(60);
/// notify_push announced something: wait this long, then fetch what changed.
const NOTIFY_REFRESH_DELAY: Duration = Duration::from_millis(300);
const POLL_TIMEOUT_S: u32 = 30;
/// An open chat asks past the server's "nothing new" memory this often: a new reaction
/// shows after this long at the latest instead of after up to 30 s (docs/chat.md „Reaktionen“).
const PEEK_EVERY: Duration = Duration::from_secs(5);
/// No answer from the server for this long (wall clock: the app may have slept): the kept
/// connections are suspect. While the app runs, the room list alone answers every 10 s.
const STALE_AFTER: Duration = Duration::from_secs(20);
/// Fetching a chat ahead of the user opening it gives up after this long (a push leaves
/// the app about ten seconds).
const CATCH_UP_TIMEOUT: Duration = Duration::from_secs(8);
/// At most this many chats are fetched ahead for one look at the room list.
const CATCH_UP_MAX: usize = 5;
/// How long a push waits for the poll of an open chat to bring what it announced.
const OPEN_CHAT_WAIT: Duration = Duration::from_secs(3);
const PAGE_SIZE: u32 = 50;
/// Messages per delta request (the server's limit for `lookIntoFuture`).
const DELTA_PAGE: usize = 100;
/// More new messages than this since the last sync: drop the cached chat instead of
/// fetching them all (the newest page is enough to show the chat).
const DELTA_MAX: usize = 500;

/// Permission bit for reactions (`react-permission` servers; Nextcloud 34+).
const PERMISSION_REACT: i64 = 256;
/// Permission bit for chatting, which also covered reactions on older servers.
const PERMISSION_CHAT: i64 = 128;

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

/// System messages about the members of a conversation (docs/groups.md).
const MEMBER_SYSTEM: &[&str] = &[
    "user_added",
    "user_removed",
    "group_added",
    "group_removed",
    "circle_added",
    "circle_removed",
    "federated_user_added",
    "federated_user_removed",
    "moderator_promoted",
    "moderator_demoted",
    "guest_moderator_promoted",
    "guest_moderator_demoted",
    "owner_promoted",
    "owner_demoted",
];

/// System messages that change the conversation itself (list entry, group settings).
const ROOM_SYSTEM: &[&str] = &[
    "conversation_renamed",
    "description_set",
    "description_removed",
    "avatar_set",
    "avatar_removed",
    "guests_allowed",
    "guests_disallowed",
    "read_only",
    "read_only_off",
    "conversation_made_public",
    "conversation_made_private",
];

/// Own text messages can be edited this long (the server's limit; none in "note to self").
const EDIT_WINDOW_S: i64 = 24 * 60 * 60;

/// System messages whose `parent` is the updated version of an existing message.
pub(crate) const UPDATING_SYSTEM: &[&str] = &[
    "reaction",
    "reaction_deleted",
    "reaction_revoked",
    "message_deleted",
    "message_edited",
];

/// ... and those of them that change what the message says: replies quote it.
const REWRITING_SYSTEM: &[&str] = &["message_deleted", "message_edited"];

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
    /// The connection state changed (docs/connection.md). A session starts as `connecting`.
    Connection { state: ConnectionState },
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
    /// Members or roles of a conversation changed (someone added, removed, promoted):
    /// reload [`Engine::members`] if they are shown. Only for open chats and own changes.
    ParticipantsChanged { token: String },
    /// Conversations with a new message that deserves a notification
    /// (docs/notifications.md). Comes after the `RoomsChanged` that brought it, also for
    /// the conversation that is open: whether to show it is the platform's call.
    /// `since_start`: the first comparison of this run, i.e. what came in while the app
    /// was closed.
    MessageNotices { items: Vec<MessageNotice>, since_start: bool },
    /// The notifications of these conversations are obsolete: read (here or elsewhere),
    /// archived or gone.
    NoticesCleared { tokens: Vec<String> },
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
    /// The user may react to messages (see docs/chat.md „Reaktionen“).
    pub can_react: bool,
    /// The user may write here, and so edit and delete own messages (docs/chat.md
    /// „Bearbeiten und Löschen“).
    pub can_chat: bool,
    /// The user may delete messages of others: moderators, in groups.
    pub can_delete_others: bool,
    pub has_call: bool,
    /// 1 always, 2 mentions only, 3 never. The server default (0) is resolved: 1 in
    /// one-to-one chats, 2 in groups (docs/settings.md).
    pub notification_level: u8,
    /// Changes whenever the avatar changes: use it as cache key.
    pub avatar_version: String,
    /// A picture was set (it can be removed).
    pub has_custom_avatar: bool,
    /// Owner or moderator: may edit the group and its members (docs/groups.md).
    pub is_moderator: bool,
    /// `false` when we are the last moderator: promote somebody first.
    pub can_leave: bool,
    pub can_delete: bool,
    /// Group link on: anyone with `group_link` can join (room type 3).
    pub is_public: bool,
    /// Our own categories it is in (docs/categories.md); use `room_sections` for the list.
    pub tag_ids: Vec<String>,
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
    /// For location messages; `text` is then its name.
    pub location: Option<GeoLocation>,
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
    /// For location messages; `text` is then its name (may be empty).
    pub location: Option<GeoLocation>,
    /// Can be deleted: by its author with `RoomView.can_chat`, by others with
    /// `RoomView.can_delete_others`.
    pub deletable: bool,
    /// Its author can edit the text (or caption) with `RoomView.can_chat`, until `edit_until`.
    pub editable: bool,
    /// Unix seconds; `None`: no limit.
    pub edit_until: Option<i64>,
    /// Who the text mentions, in its order; each stands in `text` as its `label`
    /// (docs/mentions.md). Empty for system and deleted messages.
    pub mentions: Vec<MentionView>,
    /// Somebody else mentions the user here (their name or everybody).
    pub mentions_me: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyPreview {
    pub id: i64,
    pub author: String,
    pub text: String,
    pub is_own: bool,
    pub attachment: Option<FileAttachment>,
    #[serde(default)]
    pub location: Option<GeoLocation>,
}

/// One person's reaction to a message, for the "who reacted" list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactionView {
    pub emoji: String,
    pub actor_type: String,
    pub actor_id: String,
    pub name: String,
    pub is_own: bool,
    pub timestamp: i64,
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
    /// Newer messages exist above this page: it is a window into the history
    /// (`messages_around`, `load_newer`; docs/search.md „Chat an einer Nachricht öffnen“).
    pub has_newer: bool,
    /// Pass to `load_newer`; like `oldest_id` it counts hidden messages too.
    pub newest_id: Option<i64>,
    /// Messages behind the read marker that mention the user, oldest first: the "@"
    /// button of the chat (docs/mentions.md). What the cache holds, and what this page
    /// brings: a chat with many unread messages tells them page by page.
    pub unread_mentions: Vec<i64>,
}

#[derive(Default)]
struct State {
    rooms: HashMap<String, Room>,
    modified_before: Option<i64>,
    last_full: Option<Instant>,
    chats: HashMap<String, ChatPoll>,
    rooms_task: Option<JoinHandle<()>>,
    notify_task: Option<JoinHandle<()>>,
}

/// The live poll of an open chat.
struct ChatPoll {
    task: JoinHandle<()>,
    /// The newest message id the poll has handed on: where a restart continues.
    last: Arc<AtomicI64>,
    /// The same, set once the messages are in the cache and announced.
    shown: Arc<AtomicI64>,
}

pub struct Engine {
    pub(crate) client: TalkClient,
    pub(crate) sink: Arc<dyn EventSink>,
    me: RwLock<String>,
    /// How the server calls the user ("Anna Beispiel"); the user id until it is known.
    display_name: RwLock<String>,
    /// `me` came from the server (the login name may differ from the user id).
    me_resolved: AtomicBool,
    pub(crate) conn: Mutex<crate::connection::Conn>,
    /// The state the platform was told last.
    pub(crate) conn_told: Mutex<ConnectionState>,
    /// Cuts the wait before the next retry short (network changed).
    pub(crate) retry_wake: Arc<tokio::sync::Notify>,
    /// When the server answered last, by the wall clock (it keeps running while the app
    /// sleeps, unlike `Instant`).
    answered: Mutex<Option<SystemTime>>,
    /// Ends the pause of a chat poll that failed (the network is back).
    poll_wake: Arc<tokio::sync::Notify>,
    /// One sync per chat at a time: opening a chat waits for a fetch that is under way
    /// instead of asking for the same messages again.
    sync_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    state: Mutex<State>,
    attachment_folder: RwLock<Option<String>>,
    /// Which permission bit allows reactions on this server.
    react_bit: AtomicI64,
    /// Changes of a message (reaction, edit, deletion) run one after another: quick changes
    /// of mind stay in order.
    message_lock: tokio::sync::Mutex<()>,
    /// The server lets this user create conversations (`config.conversations.can-create`).
    pub(crate) can_create: AtomicBool,
    /// Categories from the server and the locally kept collapsed state (docs/categories.md).
    pub(crate) categories: Mutex<crate::categories::Categories>,
    pub(crate) outbox: Outbox,
    pub(crate) cache: Cache,
    /// The words of the whole history, for the search (docs/search.md).
    pub(crate) search: Index,
    /// Something changed that the index may be missing.
    pub(crate) search_wake: Arc<tokio::sync::Notify>,
    pub(crate) search_task: Mutex<Option<JoinHandle<()>>>,
    /// Who can be mentioned, per chat, as the server said last (docs/mentions.md).
    pub(crate) mention_lists: Mutex<HashMap<String, crate::mentions::MentionList>>,
    pub(crate) mention_load: tokio::sync::Mutex<()>,
    pub(crate) voice: Arc<VoiceStore>,
    pub(crate) calls: CallManager,
    pub(crate) recordings: Recordings,
    notices: Mutex<Notices>,
    /// The user's status is "do not disturb": nothing announces or rings, except in
    /// conversations marked as important (the server sends no push then either).
    quiet: AtomicBool,
    /// A room refresh asked for by notify_push is running / was asked for again meanwhile.
    refreshing: AtomicBool,
    refresh_again: AtomicBool,
    /// Highest call notification id that was dealt with (missed calls are reported once).
    pub(crate) call_mark: Mutex<Option<i64>>,
    /// One look at the notifications at a time, so two triggers do not report twice.
    pub(crate) incoming_lock: tokio::sync::Mutex<()>,
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
        let cached = cache.rooms();
        let notices = Notices::load(cache.meta("notice_marks"), &cached);
        let call_mark = cache.meta("call_mark").and_then(|v| v.parse().ok());
        let state = State {
            rooms: cached.into_iter().map(|r| (r.token.clone(), r)).collect(),
            ..State::default()
        };
        Arc::new_cyclic(|weak| Self {
            client,
            sink,
            display_name: RwLock::new(cache.meta("display_name").filter(|n| !n.is_empty()).unwrap_or_else(|| me.clone())),
            me: RwLock::new(me),
            me_resolved: AtomicBool::new(false),
            conn: Mutex::default(),
            conn_told: Mutex::new(ConnectionState::Connecting),
            retry_wake: Arc::default(),
            answered: Mutex::new(None),
            poll_wake: Arc::default(),
            sync_locks: Mutex::default(),
            state: Mutex::new(state),
            attachment_folder: RwLock::new(cache.meta("attachment_folder")),
            react_bit: AtomicI64::new(
                cache.meta("react_bit").and_then(|v| v.parse().ok()).unwrap_or(PERMISSION_REACT),
            ),
            message_lock: tokio::sync::Mutex::new(()),
            can_create: AtomicBool::new(cache.meta("can_create").is_none_or(|v| v == "true")),
            categories: Mutex::new(crate::categories::Categories::load(&cache)),
            outbox: Outbox::open(data_dir.as_deref()),
            voice: Arc::new(VoiceStore::open(data_dir.as_deref())),
            search: Index::open(data_dir.as_deref()),
            search_wake: Arc::default(),
            search_task: Mutex::default(),
            mention_lists: Mutex::default(),
            mention_load: tokio::sync::Mutex::new(()),
            cache,
            calls: CallManager::default(),
            recordings: Recordings::default(),
            notices: Mutex::new(notices),
            quiet: AtomicBool::new(false),
            refreshing: AtomicBool::new(false),
            refresh_again: AtomicBool::new(false),
            call_mark: Mutex::new(call_mark),
            incoming_lock: tokio::sync::Mutex::new(()),
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
        self.await_first_answer();
        // User id, room list and server features in parallel: saves round trips on cold start.
        let (user, rooms, caps, tags, _) = tokio::join!(
            self.client.current_user(),
            self.client.rooms(None, false),
            self.client.capabilities(),
            self.client.tags(),
            self.refresh_status()
        );
        self.apply_tags(tags);
        if let Ok(caps) = caps {
            self.apply_capabilities(&caps);
        }
        if let Err(Error::Unauthorized) = &user {
            return self.guard(user).map(|_| ());
        }
        // The list first, then the connection state: while a session is `connecting`, an
        // empty list means "still loading" to the platform (docs/connection.md).
        let user = user.map(|u| self.set_user(u));
        let first = rooms.map(|batch| self.apply_rooms(batch, true));
        self.track(&user);
        self.track(&first);
        if first.is_err() && (user.is_err() || !self.list_known()) {
            // The server answered with errors only, or there is no list to show: not
            // reachable for the user either.
            self.unreachable();
        }

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
        // Calls that were missed while the app was not running (and recording notices),
        // also on servers without notify_push.
        self.spawn_incoming_check();
        let notify = tokio::spawn(Engine::notify_push_loop(Arc::downgrade(self)));
        if let Some(old) = self.state.lock().unwrap().notify_task.replace(notify) {
            old.abort();
        }
        self.spawn_search_sync();
        user.and(first)
    }

    fn apply_capabilities(&self, caps: &serde_json::Value) {
        if let Some(can) = caps.pointer("/capabilities/spreed/config/conversations/can-create").and_then(|v| v.as_bool()) {
            if self.can_create.swap(can, Ordering::Relaxed) != can {
                self.cache.set_meta("can_create", can.to_string());
            }
        }
        if caps.pointer("/capabilities/spreed").is_some() {
            self.apply_sort_order(caps.pointer("/capabilities/spreed/config/conversations/sort-order").and_then(|v| v.as_str()));
        }
        let Some(features) = caps.pointer("/capabilities/spreed/features").and_then(|f| f.as_array()) else {
            return;
        };
        self.set_categories_supported(features.iter().any(|f| f == "conversation-tags"));
        let bit = if features.iter().any(|f| f == "react-permission") {
            PERMISSION_REACT
        } else {
            PERMISSION_CHAT
        };
        if self.react_bit.swap(bit, Ordering::Relaxed) != bit {
            self.cache.set_meta("react_bit", bit.to_string());
        }
    }

    /// Resolves the real user id once (needed for WebDAV paths and own-message detection).
    pub(crate) async fn ensure_user(&self) -> Result<()> {
        if self.me_resolved.load(Ordering::Relaxed) {
            return Ok(());
        }
        let user = self.guard(self.client.current_user().await)?;
        self.set_user(user);
        Ok(())
    }

    fn set_user(&self, user: crate::client::CurrentUser) {
        let name = if user.display_name.is_empty() { user.id.clone() } else { user.display_name };
        if *self.display_name.read().unwrap() != name {
            *self.display_name.write().unwrap() = name.clone();
            self.cache.set_meta("display_name", name);
        }
        if *self.me.read().unwrap() != user.id {
            *self.me.write().unwrap() = user.id.clone();
        }
        self.cache.set_meta("user_id", user.id);
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
        // One call per process: also busy while another account rings or talks.
        if self.calls.busy_anywhere() {
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

    /// Puts one conversation (fresh from the server) into the list and the cache.
    pub(crate) fn put_room(&self, room: Room) {
        self.put_rooms(vec![room]);
    }

    /// Several conversations fresh from the server, with one `RoomsChanged`.
    pub(crate) fn put_rooms(&self, rooms: Vec<Room>) {
        if rooms.is_empty() {
            return;
        }
        self.cache.put_rooms(rooms.clone(), false);
        let views = {
            let mut st = self.state.lock().unwrap();
            for room in rooms.iter().cloned() {
                st.rooms.insert(room.token.clone(), room);
            }
            self.sorted_views(&st.rooms)
        };
        self.sink.emit(Event::RoomsChanged { rooms: views });
        self.notices_from_server(&rooms, false);
    }

    /// Sends the current list again (e.g. after a category change).
    pub(crate) fn emit_rooms(&self) {
        self.sink.emit(Event::RoomsChanged { rooms: self.rooms() });
    }

    /// Changes a cached conversation locally (optimistic UI). `edit` returns whether
    /// anything changed; only then the cache is written and `RoomsChanged` sent.
    pub(crate) fn edit_room(&self, token: &str, edit: impl FnOnce(&mut Room) -> bool) -> Result<bool> {
        let rooms = {
            let mut st = self.state.lock().unwrap();
            let Some(r) = st.rooms.get_mut(token) else {
                return Err(Error::NotFound);
            };
            if !edit(r) {
                return Ok(false);
            }
            let changed = r.clone();
            self.cache.put_rooms(vec![changed.clone()], false);
            (self.sorted_views(&st.rooms), changed)
        };
        let (rooms, changed) = rooms;
        self.sink.emit(Event::RoomsChanged { rooms });
        self.notices_local(&changed, false);
        Ok(true)
    }

    pub(crate) fn state_room(&self, token: &str) -> Option<Room> {
        self.state.lock().unwrap().rooms.get(token).cloned()
    }

    pub(crate) fn rooms_snapshot(&self) -> Vec<Room> {
        self.state.lock().unwrap().rooms.values().cloned().collect()
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

    /// [`Self::decline_call`], and waits (at most `timeout`) until the call has ended for
    /// the caller as well (1:1). For platforms that are suspended right after a decline.
    pub async fn decline_call_and_wait(&self, token: &str, timeout: Duration) {
        self.calls.decline_and_wait(token, timeout).await;
    }

    /// The incoming call of `token` stopped ringing on this device without an answer (the
    /// system ended it or refused to show it): it ends as `missed` with `CallMissed`, and
    /// the caller keeps ringing. No-op when no such call rings here.
    pub fn miss_call(&self, token: &str) {
        self.calls.miss(token);
    }

    /// [`Self::miss_call`], and waits (at most `timeout`) until the call is gone here.
    pub async fn miss_call_and_wait(&self, token: &str, timeout: Duration) {
        self.calls.miss_and_wait(token, timeout).await;
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

    /// Screen sharing on/off in the running call. Get the screen stream first (the
    /// user picks it); the core then creates `ScreenPublisher` peers for its track.
    pub fn set_screen_share(&self, on: bool) {
        self.calls.set_screen_share(on);
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
        self.stop_retries();
        if let Some(t) = self.search_task.lock().unwrap().take() {
            t.abort();
        }
        let mut st = self.state.lock().unwrap();
        if let Some(t) = st.rooms_task.take() {
            t.abort();
        }
        if let Some(t) = st.notify_task.take() {
            t.abort();
        }
        for (_, poll) in st.chats.drain() {
            poll.task.abort();
        }
    }

    pub fn user_id(&self) -> String {
        self.me.read().unwrap().clone()
    }

    /// The user's name as the server shows it, for the account list. From the cache at
    /// once, up to date after [`Self::start`]; the user id while nothing else is known.
    pub fn display_name(&self) -> String {
        self.display_name.read().unwrap().clone()
    }

    /// What this account adds to badges: the app badge sums it over all accounts, the
    /// account switcher shows it per account (docs/accounts.md).
    pub fn badge(&self) -> crate::accounts::BadgeView {
        crate::accounts::badge(&self.rooms())
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
        let batch = if full {
            // Categories change rarely: with every full refresh (docs/categories.md).
            // ... and so does the user's status ("do not disturb").
            let (batch, tags, _) = tokio::join!(self.client.rooms(since, true), self.client.tags(), self.refresh_status());
            self.apply_tags(tags);
            batch
        } else {
            self.client.rooms(since, true).await
        };
        let batch = match batch {
            Ok(batch) => batch,
            Err(e) => return self.guard(Err(e)),
        };
        // The list first, then the connection state, as at the start (docs/connection.md).
        self.apply_rooms(batch, since.is_none());
        self.reached();
        Ok(())
    }

    /// The chat list of this account is there: from the cache or from the server.
    pub(crate) fn list_known(&self) -> bool {
        let st = self.state.lock().unwrap();
        st.last_full.is_some() || !st.rooms.is_empty()
    }

    fn apply_rooms(&self, batch: crate::client::RoomsBatch, full: bool) {
        if full || !batch.rooms.is_empty() {
            self.cache.put_rooms(batch.rooms.clone(), full);
            self.index_rooms(&batch.rooms, full);
        }
        let fresh = batch.rooms.clone();
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
        self.notices_from_server(&fresh, full);
    }

    // --- new-message notices (docs/notifications.md) ---------------------------------

    /// Conversations fresh from the server: announce the ones with a new message.
    fn notices_from_server(&self, rooms: &[Room], all: bool) {
        let quiet = self.quiet.load(Ordering::Relaxed);
        let scan = self
            .notices
            .lock()
            .unwrap()
            .server(rooms, all, |m| self.is_own(&m.actor_type, &m.actor_id), quiet);
        let items: Vec<MessageNotice> = scan
            .notify
            .iter()
            .filter_map(|token| rooms.iter().find(|r| r.token == *token))
            .map(|r| MessageNotice {
                token: r.token.clone(),
                room_name: r.display_name.clone(),
                is_group: crate::notices::is_group(r),
                mention: r.unread_mention || r.unread_mention_direct,
                unread: r.unread_messages,
                message: r.last_message.as_ref().map(|m| self.preview(m)),
            })
            .collect();
        let since_start = scan.since_start;
        // What gets announced gets fetched: the chat shows the message when the user
        // follows the notification. Only where the cache is behind the room list.
        let behind: Vec<String> = scan
            .notify
            .iter()
            .filter_map(|token| rooms.iter().find(|r| r.token == *token))
            .filter(|r| {
                let newest = r.last_message.as_ref().map_or(0, |m| m.id);
                self.cache.range(&r.token).is_none_or(|range| range.newest_id < newest)
            })
            .map(|r| r.token.clone())
            .take(CATCH_UP_MAX)
            .collect();
        self.after_scan(scan);
        if !items.is_empty() {
            self.sink.emit(Event::MessageNotices { items, since_start });
        }
        self.spawn_catch_up(behind);
    }

    fn spawn_catch_up(&self, tokens: Vec<String>) {
        if tokens.is_empty() || !self.cache.enabled() {
            return;
        }
        let Some(rt) = self.runtime.get().cloned().or_else(|| tokio::runtime::Handle::try_current().ok()) else { return };
        let weak = self.weak_self();
        rt.spawn(async move {
            for token in tokens {
                let Some(engine) = weak.upgrade() else { return };
                engine.catch_up(&token).await;
            }
        });
    }

    /// A conversation changed here (read, sent, archived): its notification may be
    /// obsolete. `read_here`: the user read or answered it in this app.
    fn notices_local(&self, room: &Room, read_here: bool) {
        let scan = self.notices.lock().unwrap().local(room, read_here);
        self.after_scan(scan);
    }

    fn after_scan(&self, scan: crate::notices::Scan) {
        if scan.dirty {
            self.cache.set_meta("notice_marks", self.notices.lock().unwrap().store());
        }
        if !scan.cleared.is_empty() {
            self.sink.emit(Event::NoticesCleared { tokens: scan.cleared });
        }
    }

    /// The user's own status, for "do not disturb". Unknown (no status app, offline)
    /// keeps the last answer.
    pub(crate) async fn refresh_status(&self) {
        if let Ok(status) = self.client.user_status().await {
            self.quiet.store(status == "dnd", Ordering::Relaxed);
        }
    }

    /// "Do not disturb" applies to this conversation: no notice, no ringing.
    pub(crate) fn hushed(&self, token: &str) -> bool {
        self.quiet.load(Ordering::Relaxed) && !self.state_room(token).is_some_and(|r| r.is_important)
    }

    /// notify_push says there is a new notification: fetch what changed now instead of
    /// at the next poll. One refresh at a time; requests that come in meanwhile make it
    /// run once more (notify_push also reports our own read markers and other apps).
    pub(crate) fn spawn_rooms_refresh(&self) {
        let Some(rt) = self.runtime.get() else { return };
        if self.refreshing.swap(true, Ordering::SeqCst) {
            self.refresh_again.store(true, Ordering::SeqCst);
            return;
        }
        let weak = self.weak_self();
        rt.spawn(async move {
            loop {
                // A burst collapses into one request, and the server is done with what
                // it announces.
                tokio::time::sleep(NOTIFY_REFRESH_DELAY).await;
                let Some(engine) = weak.upgrade() else { return };
                engine.refresh_again.store(false, Ordering::SeqCst);
                let _ = engine.refresh_rooms(false).await;
                if !engine.refresh_again.load(Ordering::SeqCst) {
                    engine.refreshing.store(false, Ordering::SeqCst);
                    // Asked again in this very moment: the regular poll picks it up.
                    return;
                }
            }
        });
    }

    /// Asks the server for a test push to this user's devices ("Push testen").
    pub async fn test_push(&self) -> Result<()> {
        self.guard(self.client.test_push().await).map(|_| ())
    }

    /// Newest messages from the cache (empty without one), for the first frame of a chat.
    /// Call [`Self::open_chat`] afterwards for the server state.
    pub fn cached_chat(&self, token: &str) -> ChatPage {
        let Some(range) = self.cache.range(token) else {
            return ChatPage::default();
        };
        let (messages, more) = self.cache.messages(token, None, PAGE_SIZE);
        self.with_unread_mentions(token, self.cached_page(messages, more || range.has_more, range.last_common_read))
    }

    /// Brings a chat up to date and starts live updates for it. Returns its newest page.
    /// With a cache only the messages since the last sync are fetched (edits, deletions
    /// and reactions arrive among them as system messages).
    ///
    /// Offline, the error is returned but live updates still start from the cached state,
    /// so the chat catches up once the server is reachable.
    pub async fn open_chat(self: &Arc<Self>, token: &str) -> Result<ChatPage> {
        // Opened right after a pause (from a notification): not on a dead connection.
        self.revive();
        self.warm_mentions(token);
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
                self.cache.flush().await;
                Ok(self.with_unread_mentions(token, page))
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
        // Under the lock: the poll finds itself in the map when its first answer comes.
        let mut st = self.state.lock().unwrap();
        if let Some(old) = st.chats.insert(token.to_string(), self.spawn_poll(token, newest)) {
            old.task.abort();
        }
    }

    fn spawn_poll(self: &Arc<Self>, token: &str, newest: i64) -> ChatPoll {
        let weak = Arc::downgrade(self);
        let tok = token.to_string();
        let last = Arc::new(AtomicI64::new(newest));
        let shown = Arc::new(AtomicI64::new(newest));
        let (seen, done) = (last.clone(), shown.clone());
        let task = tokio::spawn(async move { poll_loop(weak, tok, seen, done).await });
        ChatPoll { task, last, shown }
    }

    /// Whether this poll is still the one of its chat, and if so, how far it has come. A
    /// poll that was replaced while its answer arrived hands nothing on: its successor
    /// asks for the same messages, and they must not show up twice.
    fn poll_goes_on(&self, token: &str, seen: &Arc<AtomicI64>, last: i64) -> bool {
        let st = self.state.lock().unwrap();
        let current = st.chats.get(token).is_some_and(|poll| Arc::ptr_eq(&poll.last, seen));
        if current {
            seen.store(last, Ordering::SeqCst);
        }
        current
    }

    pub fn close_chat(&self, token: &str) {
        if let Some(poll) = self.state.lock().unwrap().chats.remove(token) {
            poll.task.abort();
        }
    }

    /// The polls of the open chats start over where they are: a long poll that hangs on a
    /// dead connection would otherwise wait for its timeout (45 s) before it notices.
    fn restart_polls(self: &Arc<Self>) {
        let mut st = self.state.lock().unwrap();
        for (token, poll) in st.chats.iter_mut() {
            poll.task.abort();
            *poll = self.spawn_poll(token, poll.last.load(Ordering::SeqCst));
        }
    }

    /// After a pause without any answer (the app slept, the network was gone) the kept
    /// connections may be dead without anything saying so: new ones from here on, and the
    /// open chats ask again at once. Does nothing while the server keeps answering.
    pub(crate) fn revive(self: &Arc<Self>) {
        {
            let mut answered = self.answered.lock().unwrap();
            let now = SystemTime::now();
            // Never answered: nothing is pooled yet. A clock that went backwards counts as a pause.
            let stale = answered.is_some_and(|t| now.duration_since(t).map_or(true, |d| d >= STALE_AFTER));
            if !stale {
                return;
            }
            // Once per pause.
            *answered = Some(now);
        }
        self.client.fresh_connections();
        if tokio::runtime::Handle::try_current().is_ok() {
            self.restart_polls();
        } else if let Some(rt) = self.runtime.get() {
            let _in_runtime = rt.enter();
            self.restart_polls();
        }
    }

    /// For tests: as if the server had not answered for a long while (the app slept).
    #[doc(hidden)]
    pub fn pretend_paused(&self) {
        *self.answered.lock().unwrap() = Some(SystemTime::UNIX_EPOCH);
    }

    /// The network is back or the app returned: a chat poll that pauses after a failure
    /// tries again now, and after a longer pause everything starts on new connections.
    pub(crate) fn wake_chats(&self) {
        if let Some(engine) = self.weak.upgrade() {
            engine.revive();
        }
        self.poll_wake.notify_waiters();
    }

    /// Something new was announced for this chat (a push, a notice): have it in the cache
    /// before the user opens the chat, so its first frame shows the message. An open chat
    /// gets it from its poll. Quiet about failures: opening the chat asks again.
    ///
    /// Returns when the message is there or the wait is over: a platform that is only awake
    /// for the push (iOS in the background) stays awake as long as this runs.
    pub(crate) async fn catch_up(self: &Arc<Self>, token: &str) {
        let before = self.poll_seen(token);
        self.revive();
        self.poll_wake.notify_waiters();
        if let Some(before) = before {
            // Ask whether anything is left to wait for: the poll may have brought it before
            // the push arrived, and then nothing more will come.
            let _ = tokio::time::timeout(OPEN_CHAT_WAIT, async {
                let newest = match self.fetch_delta(token, before).await {
                    Ok(Some((messages, _))) => messages.iter().map(|m| m.id).max(),
                    // Too much to tell, or no answer: the poll decides.
                    _ => Some(i64::MAX),
                };
                let Some(newest) = newest else { return };
                while self.poll_seen(token).is_some_and(|seen| seen == before && seen < newest) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await;
            return;
        }
        if !self.cache.enabled() {
            return;
        }
        let _ = tokio::time::timeout(CATCH_UP_TIMEOUT, self.sync_chat(token)).await;
    }

    /// The newest message id the poll of an open chat has delivered; `None` for a closed chat.
    fn poll_seen(&self, token: &str) -> Option<i64> {
        self.state.lock().unwrap().chats.get(token).map(|poll| poll.shown.load(Ordering::SeqCst))
    }

    fn sync_lock(&self, token: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.sync_locks.lock().unwrap().entry(token.to_string()).or_default().clone()
    }

    /// Whether the chat is being live-polled (its echoes will arrive).
    pub(crate) fn chat_open(&self, token: &str) -> bool {
        self.state.lock().unwrap().chats.contains_key(token)
    }

    /// Fetches the delta since the cached state (or the newest page) into the cache and
    /// returns the newest page from it plus the newest message id.
    async fn sync_chat(&self, token: &str) -> Result<(ChatPage, i64)> {
        let lock = self.sync_lock(token);
        let _one_at_a_time = lock.lock().await;
        let cached = self.cache.range(token);
        let delta = match cached {
            Some(r) => self.fetch_delta(token, r.newest_id).await?,
            None => None,
        };
        let fresh = match delta {
            Some((messages, last_common_read)) => {
                let updates = parent_updates(&messages);
                // Everything after the cached range, without a gap.
                let cover = cached.zip(messages.iter().map(|m| m.id).max()).map(|(r, newest)| (r.newest_id + 1, newest));
                self.index_messages(token, &messages, &updates, cover);
                self.cache.append(token, messages, updates, last_common_read);
                None
            }
            None => {
                let batch = self.guard(self.client.history(token, None, PAGE_SIZE).await)?;
                let has_more = batch.messages.len() as u32 >= PAGE_SIZE;
                let ids = batch.messages.iter().map(|m| m.id);
                self.index_messages(token, &batch.messages, &parent_updates(&batch.messages), ids.clone().min().zip(ids.max()));
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
            let batch = self.guard(self.client.newer_messages(token, last, DELTA_PAGE).await)?;
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
                return Ok(self.with_unread_mentions(token, self.cached_page(messages, more || r.has_more, None)));
            }
        }
        let batch = self.guard(self.client.history(token, Some(before_id), PAGE_SIZE).await)?;
        let has_more = batch.messages.len() as u32 >= PAGE_SIZE;
        let cover = batch.messages.iter().map(|m| m.id).min().map(|oldest| (oldest, before_id - 1));
        self.index_messages(token, &batch.messages, &parent_updates(&batch.messages), cover);
        self.cache.prepend(token, before_id, batch.messages.clone(), has_more);
        Ok(self.with_unread_mentions(token, self.page(batch.messages, batch.last_common_read)))
    }

    /// A window into the history around a message (a search hit, a quoted message): up to
    /// [`PAGE_SIZE`] messages before it, the message, and as many after it. From the cache
    /// when it has the message, else from the server. `load_older` and `load_newer`
    /// continue from `oldest_id` and `newest_id`; live updates belong to the end of the
    /// chat, which `cached_chat` shows again (docs/search.md).
    pub async fn messages_around(&self, token: &str, message_id: i64) -> Result<ChatPage> {
        self.cache.flush().await;
        if let Some(r) = self.cache.range(token).filter(|r| (r.oldest_id..=r.newest_id).contains(&message_id)) {
            let (mut messages, more) = self.cache.messages(token, Some(message_id + 1), PAGE_SIZE + 1);
            if messages.last().is_some_and(|m| m.id == message_id) {
                let (after, newer) = self.cache.messages_after(token, message_id, PAGE_SIZE);
                messages.extend(after);
                let mut page = self.cached_page(messages, more || r.has_more, r.last_common_read);
                page.has_newer = newer;
                return Ok(self.with_unread_mentions(token, page));
            }
        }
        let batch = self.guard(self.client.message_context(token, message_id, PAGE_SIZE).await)?;
        let ids = batch.messages.iter().map(|m| m.id);
        let (older, newer) = (ids.clone().filter(|id| *id < message_id).count(), ids.clone().filter(|id| *id > message_id).count());
        self.index_messages(token, &batch.messages, &parent_updates(&batch.messages), ids.clone().min().zip(ids.max()));
        let mut page = self.page(batch.messages, batch.last_common_read);
        page.has_more = older as u32 >= PAGE_SIZE;
        page.has_newer = newer as u32 >= PAGE_SIZE;
        Ok(self.with_unread_mentions(token, page))
    }

    /// Messages newer than `after_id`, for a window into the history: from the cache when
    /// it reaches that far back, else from the server. `has_newer` turns false at the end
    /// of the chat; from there on the live updates continue it.
    pub async fn load_newer(&self, token: &str, after_id: i64) -> Result<ChatPage> {
        self.cache.flush().await;
        if let Some(r) = self.cache.range(token).filter(|r| after_id >= r.oldest_id) {
            let (messages, newer) = self.cache.messages_after(token, after_id, PAGE_SIZE);
            let mut page = self.cached_page(messages, true, r.last_common_read);
            page.has_newer = newer;
            return Ok(self.with_unread_mentions(token, page));
        }
        let batch = self.guard(self.client.newer_messages(token, after_id, PAGE_SIZE as usize).await)?;
        let newest = batch.messages.iter().map(|m| m.id).max().into_iter().chain(batch.last_given).max();
        self.index_messages(token, &batch.messages, &parent_updates(&batch.messages), newest.map(|n| (after_id + 1, n)));
        let full = batch.messages.len() as u32 >= PAGE_SIZE;
        // What the cache holds follows at some point: the window is not at the end before that.
        let below_cache = self.cache.range(token).is_some_and(|r| newest.is_none_or(|n| n < r.newest_id));
        let mut page = self.page(batch.messages, batch.last_common_read);
        page.has_more = true;
        page.has_newer = full || below_cache;
        page.newest_id = newest.or(Some(after_id));
        Ok(self.with_unread_mentions(token, page))
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
        let (rooms, changed) = {
            let mut st = self.state.lock().unwrap();
            let changed = st.rooms.get_mut(token).map(|room| {
                room.last_activity = room.last_activity.max(msg.timestamp);
                room.last_message = Some(msg);
                room.unread_messages = 0;
                room.unread_mention = false;
                self.cache.put_rooms(vec![room.clone()], false);
                room.clone()
            });
            (self.sorted_views(&st.rooms), changed)
        };
        self.sink.emit(Event::RoomsChanged { rooms });
        if let Some(room) = changed {
            self.notices_local(&room, true);
        }
    }

    /// A message we sent from the outbox: show it in the chat and the room list.
    pub(crate) fn note_own_message(&self, token: &str, msg: ChatMessage) {
        self.sink.emit(Event::MessagesAdded {
            token: token.to_string(),
            messages: vec![self.view(&msg)],
        });
        self.update_room_after_send(token, msg);
    }

    // --- reactions (docs/chat.md „Reaktionen“) ---------------------------------------------------

    /// Sets the own reaction to a message like Signal: one per person. `Some(emoji)`
    /// replaces the own reaction(s) with `emoji`, or removes it when it is already the own
    /// one; `None` removes the own reaction(s).
    ///
    /// The change shows at once (`MessagesUpdated`), then the server's answer replaces
    /// it. On failure the message falls back to what the server has and the error is
    /// returned (see [`reaction_error_text`]).
    pub async fn set_reaction(&self, token: &str, message_id: i64, emoji: Option<&str>) -> Result<()> {
        let _order = self.message_lock.lock().await;
        // A message that just arrived may still be on its way into the cache.
        self.cache.flush().await;
        let cached = self.cache.message(token, message_id);
        let own: Vec<String> = match &cached {
            Some(m) => m.reactions_self.clone(),
            // Not cached (e.g. CLI without data dir): ask the server what is ours.
            None => self.own_reactions(&self.guard(self.client.reactions(token, message_id).await)?),
        };
        let add = emoji.filter(|e| !own.iter().any(|o| o == e));
        if let Some(m) = &cached {
            self.publish_message(token, optimistic(m, &own, add));
        }
        let mut result: Option<Reactions> = None;
        let outcome: Result<()> = async {
            for old in &own {
                match self.guard(self.client.remove_reaction(token, message_id, old).await) {
                    Ok(r) => result = Some(r),
                    Err(Error::NotFound) => {} // already gone
                    Err(e) => return Err(e),
                }
            }
            if let Some(e) = add {
                result = Some(self.guard(self.client.add_reaction(token, message_id, e).await)?);
            }
            Ok(())
        }
        .await;
        if let Some(m) = cached {
            let truth = match (&outcome, result) {
                (Ok(()), Some(r)) => Some(r),
                // Nothing confirmed by the server: ask it (or show the old state).
                _ => self.client.reactions(token, message_id).await.ok(),
            };
            let updated = match truth {
                Some(r) => self.with_reactions(&m, &r),
                None if outcome.is_err() => m,
                None => return outcome,
            };
            self.publish_message(token, updated);
        }
        outcome
    }

    /// Who reacted to a message: own reactions first, then newest first.
    pub async fn reactions(&self, token: &str, message_id: i64) -> Result<Vec<ReactionView>> {
        let all = self.guard(self.client.reactions(token, message_id).await)?;
        let mut list: Vec<ReactionView> = all
            .0
            .into_iter()
            .flat_map(|(emoji, people)| {
                people.into_iter().map(move |p| (emoji.clone(), p))
            })
            .map(|(emoji, p)| ReactionView {
                is_own: self.is_own(&p.actor_type, &p.actor_id),
                emoji,
                actor_type: p.actor_type,
                actor_id: p.actor_id,
                name: p.actor_display_name,
                timestamp: p.timestamp,
            })
            .collect();
        list.sort_by(|a, b| b.is_own.cmp(&a.is_own).then(b.timestamp.cmp(&a.timestamp)));
        Ok(list)
    }

    fn own_reactions(&self, all: &Reactions) -> Vec<String> {
        all.0
            .iter()
            .filter(|(_, people)| people.iter().any(|p| self.is_own(&p.actor_type, &p.actor_id)))
            .map(|(e, _)| e.clone())
            .collect()
    }

    /// `m` with the reactions the server reported.
    fn with_reactions(&self, m: &ChatMessage, all: &Reactions) -> ChatMessage {
        let mut m = m.clone();
        m.reactions_self = self.own_reactions(all);
        m.reactions = all
            .0
            .iter()
            .filter(|(_, people)| !people.is_empty())
            .map(|(e, people)| (e.clone(), people.len() as i64))
            .collect();
        m
    }

    // --- edit and delete (docs/chat.md „Bearbeiten und Löschen“) ---------------------------------

    /// The text of a message for the input field, with its mentions: handed back to
    /// [`Self::edit_message`] they are still mentions after saving (docs/mentions.md).
    /// `None` when the message is not cached (the platform then starts from
    /// `MessageView.text` and `MessageView.mentions` is all it has).
    pub async fn edit_source(&self, token: &str, message_id: i64) -> Option<EditDraft> {
        self.cache.flush().await;
        let m = self.cache.message(token, message_id).filter(|m| !m.is_deleted())?;
        Some(EditDraft { text: m.caption_text(), mentions: m.mention_picks() })
    }

    /// Replaces the text of an own message (the caption of an own file). The new version
    /// arrives as `MessagesUpdated` before this returns; nothing shows before the server
    /// has it. See [`message_error_text`] for the error.
    /// `mentions`: what the user picked from the suggestions, and what `edit_source` gave.
    pub async fn edit_message(&self, token: &str, message_id: i64, text: &str, mentions: &[MentionPick]) -> Result<()> {
        let text = crate::mentions::wire_text(text.trim(), mentions);
        let text = text.as_str();
        // Pending messages have no id on the server yet.
        if message_id <= 0 {
            return Err(Error::NotFound);
        }
        let _order = self.message_lock.lock().await;
        self.cache.flush().await;
        let before = self.cache.message(token, message_id);
        if let Some(m) = &before {
            if m.is_deleted() {
                return Err(Error::NotFound);
            }
            if m.source_text().trim() == text {
                return Ok(());
            }
        }
        let answer = self.guard(self.client.edit_message(token, message_id, text).await)?;
        self.apply_answer(token, answer, before.as_ref());
        Ok(())
    }

    /// Deletes a message for everybody: an own one, or as a moderator of a group anyone's.
    /// The deleted message arrives as `MessagesUpdated` before this returns.
    pub async fn delete_message(&self, token: &str, message_id: i64) -> Result<()> {
        if message_id <= 0 {
            return Err(Error::NotFound);
        }
        let _order = self.message_lock.lock().await;
        self.cache.flush().await;
        let before = self.cache.message(token, message_id);
        // Deleting it twice is an error to the server.
        if before.as_ref().is_some_and(ChatMessage::is_deleted) {
            return Ok(());
        }
        let answer = self.guard(self.client.delete_message(token, message_id).await)?;
        self.apply_answer(token, answer, before.as_ref());
        Ok(())
    }

    /// The server's answer to an own edit or deletion: its `parent` is the new version.
    /// Without one the live update brings it.
    fn apply_answer(&self, token: &str, answer: ChatMessage, before: Option<&ChatMessage>) {
        let Some(mut m) = answer.parent.map(|p| *p) else { return };
        // The answer leaves out which reactions are ours.
        if let Some(old) = before {
            m.reactions_self = old.reactions_self.iter().filter(|e| m.reactions.contains_key(*e)).cloned().collect();
        }
        let changed = [m.id];
        let mut updates = vec![m];
        let messages = self.update_views(token, &mut updates, &changed);
        self.index_messages(token, &[], &updates, None);
        self.cache.append(token, Vec::new(), updates.clone(), None);
        self.sink.emit(Event::MessagesUpdated { token: token.to_string(), messages });
        let rooms = {
            let mut st = self.state.lock().unwrap();
            self.update_preview(&mut st, token, &updates).then(|| self.sorted_views(&st.rooms))
        };
        if let Some(rooms) = rooms {
            self.sink.emit(Event::RoomsChanged { rooms });
        }
    }

    /// The views of new versions of messages, completed in place with what the server
    /// leaves out: a changed message comes without its quote. For the ids in `rewritten`
    /// (edited, deleted) the cached replies follow, with the new quote.
    fn update_views(&self, token: &str, updates: &mut Vec<ChatMessage>, rewritten: &[i64]) -> Vec<MessageView> {
        // The last version of each message.
        let mut seen = std::collections::HashSet::new();
        let mut last: Vec<ChatMessage> = updates.drain(..).rev().filter(|m| seen.insert(m.id)).collect();
        last.reverse();
        *updates = last;
        for m in updates.iter_mut() {
            if m.parent.is_none() {
                m.parent = self.cache.message(token, m.id).and_then(|old| old.parent);
            }
        }
        // A quoted message that changed with it.
        let quotes: HashMap<i64, ChatMessage> =
            updates.iter().map(|m| (m.id, ChatMessage { parent: None, ..m.clone() })).collect();
        for m in updates.iter_mut() {
            if let Some(q) = m.parent.as_ref().and_then(|p| quotes.get(&p.id)) {
                m.parent = Some(Box::new(q.clone()));
            }
        }
        let unlimited = self.edit_unlimited(token);
        let mut views: Vec<MessageView> = updates.iter().map(|m| self.view_in(m, unlimited)).collect();
        for id in rewritten {
            let Some(quote) = quotes.get(id) else { continue };
            for mut reply in self.cache.replies_to(token, *id) {
                if quotes.contains_key(&reply.id) || !is_visible(&reply) {
                    continue;
                }
                reply.parent = Some(Box::new(quote.clone()));
                views.push(self.view_in(&reply, unlimited));
            }
        }
        views
    }

    /// Replaces the last message of the room in the list when it is one of `updates`.
    fn update_preview(&self, st: &mut State, token: &str, updates: &[ChatMessage]) -> bool {
        let Some(room) = st.rooms.get_mut(token) else { return false };
        let Some(new) = room.last_message.as_ref().and_then(|l| updates.iter().find(|m| m.id == l.id)) else {
            return false;
        };
        room.last_message = Some(new.clone());
        self.cache.put_rooms(vec![room.clone()], false);
        true
    }

    /// Stores a changed message and shows it.
    fn publish_message(&self, token: &str, m: ChatMessage) {
        let view = self.view(&m);
        self.index_messages(token, &[], std::slice::from_ref(&m), None);
        self.cache.append(token, Vec::new(), vec![m], None);
        self.sink.emit(Event::MessagesUpdated {
            token: token.to_string(),
            messages: vec![view],
        });
    }

    pub async fn mark_read(&self, token: &str, message_id: i64) -> Result<()> {
        self.mark_read_changed(token, message_id).await.map(|_| ())
    }

    /// [`Self::mark_read`]; says whether there was anything to mark (then the list
    /// changed and the conversation's notification was cleared).
    async fn mark_read_changed(&self, token: &str, message_id: i64) -> Result<bool> {
        let changed = {
            let mut st = self.state.lock().unwrap();
            match st.rooms.get_mut(token) {
                Some(r) if r.unread_messages > 0 || r.last_read_message < message_id => {
                    r.unread_messages = 0;
                    r.unread_mention = false;
                    r.last_read_message = r.last_read_message.max(message_id);
                    let room = r.clone();
                    self.cache.put_rooms(vec![room.clone()], false);
                    Some((self.sorted_views(&st.rooms), room))
                }
                _ => None,
            }
        };
        let Some((rooms, room)) = changed else { return Ok(false) };
        self.sink.emit(Event::RoomsChanged { rooms });
        self.notices_local(&room, true);
        self.guard(self.client.set_read_marker(token, message_id).await).map(|_| true)
    }

    /// "Als gelesen markieren" from the chat list or a notification: read marker on the
    /// newest message. The newest message is asked from the server: the notification may
    /// show one the cached list does not know yet (a push that started the app).
    pub async fn mark_room_read(&self, token: &str) -> Result<()> {
        let Some(cached) = self.state_room(token) else {
            return Err(Error::NotFound);
        };
        let room = self.client.room(token).await.unwrap_or(cached);
        let newest = room.last_message.as_ref().map_or(room.last_read_message, |m| m.id);
        let marked = self.mark_read_changed(token, newest).await;
        // Whatever the list said before: the user is done with this conversation.
        if !matches!(marked, Ok(true)) {
            self.sink.emit(Event::NoticesCleared { tokens: vec![token.to_string()] });
        }
        marked.map(|_| ())
    }

    /// Signs out on the server; call `shutdown` afterwards.
    pub async fn logout(&self) -> Result<()> {
        self.shutdown();
        self.outbox.clear();
        self.cache.clear();
        self.search.clear();
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
        self.track(&r);
        r
    }

    fn track<T>(&self, r: &Result<T>) {
        // Any answer, an error from the server too, shows that the connection lives.
        if !matches!(r, Err(Error::Network(_))) {
            *self.answered.lock().unwrap() = Some(SystemTime::now());
        }
        match r {
            Ok(_) => self.reached(),
            Err(Error::Unauthorized) => self.sink.emit(Event::AuthFailed),
            Err(Error::Network(_)) => self.unreachable(),
            Err(_) => {}
        }
    }

    pub(crate) fn is_own(&self, actor_type: &str, actor_id: &str) -> bool {
        actor_type == "users" && *self.me.read().unwrap() == actor_id
    }

    fn page(&self, messages: Vec<ChatMessage>, last_common_read: Option<i64>) -> ChatPage {
        let has_more = messages.len() as u32 >= PAGE_SIZE;
        let oldest_id = messages.iter().map(|m| m.id).min();
        let unlimited = messages.first().is_some_and(|m| self.edit_unlimited(&m.token));
        ChatPage {
            oldest_id,
            messages: messages
                .iter()
                .filter(|m| is_visible(m))
                .map(|m| self.view_in(m, unlimited))
                .collect(),
            has_more,
            last_common_read,
            reset: false,
            has_newer: false,
            newest_id: messages.iter().map(|m| m.id).max(),
            unread_mentions: Vec::new(),
        }
    }

    /// A page of cached messages (already known to be in the cache's range).
    fn cached_page(&self, messages: Vec<ChatMessage>, has_more: bool, last_common_read: Option<i64>) -> ChatPage {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let unlimited = messages.first().is_some_and(|m| self.edit_unlimited(&m.token));
        ChatPage {
            oldest_id: messages.iter().map(|m| m.id).min(),
            messages: messages
                .iter()
                .filter(|m| is_visible(m) && !(m.expiration_timestamp > 0 && m.expiration_timestamp < now))
                .map(|m| self.view_in(m, unlimited))
                .collect(),
            has_more,
            last_common_read,
            reset: false,
            has_newer: false,
            newest_id: messages.iter().map(|m| m.id).max(),
            unread_mentions: Vec::new(),
        }
    }

    /// Own messages can be edited without a time limit here ("note to self").
    fn edit_unlimited(&self, token: &str) -> bool {
        let st = self.state.lock().unwrap();
        st.rooms.get(token).is_some_and(|r| r.room_type == RoomType::NoteToSelf)
    }

    /// Not with `state` locked: it looks up the room.
    pub fn view(&self, m: &ChatMessage) -> MessageView {
        self.view_in(m, self.edit_unlimited(&m.token))
    }

    fn view_in(&self, m: &ChatMessage, edit_unlimited: bool) -> MessageView {
        let deleted = m.is_deleted();
        let is_own = self.is_own(&m.actor_type, &m.actor_id);
        // On the server, and neither a system message nor deleted.
        let deletable = m.id > 0 && !deleted && !m.is_system();
        let mentions = match deleted || m.is_system() {
            true => Vec::new(),
            false => m.mentions(&self.me.read().unwrap()),
        };
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
            is_own,
            edited: m.last_edit_timestamp.is_some_and(|t| t > 0),
            markdown: m.markdown,
            // A deleted message says nothing, so it quotes nothing either.
            reply_to: m.parent.as_deref().filter(|_| !deleted).map(|p| ReplyPreview {
                id: p.id,
                author: p.actor_display_name.clone(),
                text: if p.message_type == "comment_deleted" {
                    String::new()
                } else {
                    preview_text(&p.caption_text())
                },
                is_own: self.is_own(&p.actor_type, &p.actor_id),
                attachment: p.attachment(),
                location: p.location(),
            }),
            reactions: m.reactions.clone(),
            reactions_self: m.reactions_self.clone(),
            attachment: if deleted { None } else { m.attachment() },
            location: if deleted { None } else { m.location() },
            deletable,
            // Text and files (their caption); no voice message, poll or location.
            editable: deletable
                && is_own
                && m.message_type == "comment"
                && !m.message_parameters.contains_key("object"),
            edit_until: (!edit_unlimited).then_some(m.timestamp + EDIT_WINDOW_S),
            mentions_me: !is_own && mentions.iter().any(|x| x.is_self),
            mentions,
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

    /// A message as one line, for the conversation list and for notifications.
    fn preview(&self, m: &ChatMessage) -> MessagePreview {
        MessagePreview {
            author: m.actor_display_name.clone(),
            text: if m.message_type == "comment_deleted" {
                String::new()
            } else {
                preview_text(&m.caption_text())
            },
            is_own: self.is_own(&m.actor_type, &m.actor_id),
            is_system: m.is_system(),
            attachment: m.attachment(),
            location: m.location(),
        }
    }

    fn room_view(&self, r: &Room) -> RoomView {
        let can_chat = r.read_only == 0 && (r.permissions == 0 || r.permissions & PERMISSION_CHAT != 0);
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
            last_message: r.last_message.as_ref().map(|m| self.preview(m)),
            is_favorite: r.is_favorite,
            is_archived: r.is_archived,
            read_only: r.read_only != 0,
            can_react: r.read_only == 0
                && (r.permissions == 0 || r.permissions & self.react_bit.load(Ordering::Relaxed) != 0),
            can_chat,
            // In a one-to-one chat both are "owner": the server refuses it there.
            can_delete_others: can_chat
                && matches!(r.participant_type, 1 | 2 | 6)
                && matches!(r.room_type, RoomType::Group | RoomType::Public),
            has_call: r.has_call,
            notification_level: effective_notification_level(r.notification_level, r.room_type),
            avatar_version: r.avatar_version.clone(),
            has_custom_avatar: r.is_custom_avatar,
            is_moderator: matches!(r.participant_type, 1 | 2 | 6),
            can_leave: r.can_leave_conversation,
            can_delete: r.can_delete_conversation,
            is_public: r.room_type == RoomType::Public,
            tag_ids: r.tag_ids.clone(),
        }
    }

    /// Applies a poll result: emits new and updated messages, refreshes the room preview.
    /// Returns whether the conversation itself changed (name, members …): then the
    /// caller refreshes the room list.
    /// `after`: the id the poll asked from, so the batch is everything that follows it.
    fn apply_live(&self, token: &str, after: i64, messages: Vec<ChatMessage>) -> bool {
        let members = messages.iter().any(|m| MEMBER_SYSTEM.contains(&m.system_message.as_str()));
        let room = members || messages.iter().any(|m| ROOM_SYSTEM.contains(&m.system_message.as_str()));
        if members {
            self.mentions_stale(token);
            self.sink.emit(Event::ParticipantsChanged { token: token.to_string() });
        }
        let mut updates = parent_updates(&messages);
        // A message that came and changed in the same batch has its quote right there.
        for u in updates.iter_mut().filter(|u| u.parent.is_none()) {
            u.parent = messages.iter().find(|m| m.id == u.id).and_then(|m| m.parent.clone());
        }
        let rewritten: Vec<i64> = messages
            .iter()
            .filter(|m| REWRITING_SYSTEM.contains(&m.system_message.as_str()))
            .filter_map(|m| m.parent.as_ref().map(|p| p.id))
            .collect();
        let updated = self.update_views(token, &mut updates, &rewritten);
        let cover = messages.iter().map(|m| m.id).max().map(|newest| (after + 1, newest));
        self.index_messages(token, &messages, &updates, cover);
        if messages.iter().any(|m| m.system_message == "history_cleared") {
            // Forget the cached copy; the next open loads the remaining history.
            self.cache.replace_chat(token, Vec::new(), false, None);
        } else if !messages.is_empty() {
            self.cache.append(token, messages.clone(), updates.clone(), None);
        }
        let unlimited = self.edit_unlimited(token);
        let mut added = Vec::new();
        let mut latest: Option<ChatMessage> = None;
        for m in messages {
            if is_visible(&m) {
                added.push(self.view_in(&m, unlimited));
                latest = Some(m);
            }
        }
        if !updated.is_empty() {
            self.sink.emit(Event::MessagesUpdated {
                token: token.to_string(),
                messages: updated,
            });
        }
        if !added.is_empty() {
            let refs: Vec<String> = added.iter().map(|m| m.reference_id.clone()).collect();
            self.sink.emit(Event::MessagesAdded {
                token: token.to_string(),
                messages: added,
            });
            // After the message itself, so the pending bubble is replaced without a gap.
            self.outbox_echo(token, &refs.iter().map(String::as_str).collect::<Vec<_>>());
        }
        let rooms = {
            let mut st = self.state.lock().unwrap();
            // The server does not touch the room for an edit or a deletion: the list
            // would show the old text until its next full refresh.
            let mut changed = self.update_preview(&mut st, token, &updates);
            if let (Some(m), Some(room)) = (latest, st.rooms.get_mut(token)) {
                room.last_activity = room.last_activity.max(m.timestamp);
                room.last_message = Some(m);
                self.cache.put_rooms(vec![room.clone()], false);
                changed = true;
            }
            changed.then(|| self.sorted_views(&st.rooms))
        };
        if let Some(rooms) = rooms {
            self.sink.emit(Event::RoomsChanged { rooms });
        }
        room
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

/// `m` as it will look once the own reactions `own` are removed and `add` is added.
fn optimistic(m: &ChatMessage, own: &[String], add: Option<&str>) -> ChatMessage {
    let mut m = m.clone();
    for e in own {
        if let Some(n) = m.reactions.get_mut(e) {
            *n -= 1;
        }
    }
    if let Some(e) = add {
        *m.reactions.entry(e.to_string()).or_insert(0) += 1;
    }
    m.reactions.retain(|_, n| *n > 0);
    m.reactions_self = add.map(|e| vec![e.to_string()]).unwrap_or_default();
    m
}

/// German text for a failed [`Engine::set_reaction`], for a short notice.
pub fn reaction_error_text(e: &Error) -> String {
    match e {
        Error::Http { status: 403, .. } => "Keine Berechtigung für Reaktionen".into(),
        Error::NotFound => "Nachricht nicht gefunden".into(),
        Error::Network(_) | Error::RetryLater { .. } => "Keine Verbindung – Reaktion nicht gesendet".into(),
        _ => "Reaktion fehlgeschlagen".into(),
    }
}

/// Which change of a message failed, for [`message_error_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageOp {
    Edit,
    Delete,
}

/// German text for a failed [`Engine::edit_message`] or [`Engine::delete_message`], for a
/// short notice.
pub fn message_error_text(op: MessageOp, e: &Error) -> String {
    let text = match (op, e) {
        (MessageOp::Edit, Error::Network(_) | Error::RetryLater { .. }) => "Keine Verbindung – Nachricht nicht geändert",
        (MessageOp::Delete, Error::Network(_) | Error::RetryLater { .. }) => "Keine Verbindung – Nachricht nicht gelöscht",
        (MessageOp::Edit, Error::Http { status: 400, .. }) if e.ocs_error().as_deref() == Some("age") => {
            "Nachrichten lassen sich nur 24 Stunden lang bearbeiten"
        }
        (MessageOp::Edit, Error::Http { status: 400, .. }) => "Die Nachricht darf nicht leer sein",
        (_, Error::Http { status: 413, .. }) => "Die Nachricht ist zu lang",
        (_, Error::Http { status: 403, .. }) => "Keine Berechtigung",
        (MessageOp::Edit, Error::Http { status: 405, .. }) => "Diese Nachricht lässt sich nicht bearbeiten",
        (MessageOp::Delete, Error::Http { status: 405, .. }) => "Diese Nachricht lässt sich nicht löschen",
        (_, Error::Http { status: 412, .. }) => "Der Chat ist gerade gesperrt",
        (_, Error::NotFound) => "Nachricht nicht gefunden",
        (MessageOp::Edit, _) => "Bearbeiten fehlgeschlagen",
        (MessageOp::Delete, _) => "Löschen fehlgeschlagen",
    };
    text.into()
}

/// New versions of earlier messages carried by edit/deletion/reaction system messages.
pub(crate) fn parent_updates(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter(|m| UPDATING_SYSTEM.contains(&m.system_message.as_str()))
        .filter_map(|m| m.parent.as_deref().cloned())
        .collect()
}

/// `seen`: the newest message id handed on, kept outside so a restart continues there.
/// `shown`: the same once the batch is applied, for whoever waits for the message.
async fn poll_loop(engine: std::sync::Weak<Engine>, token: String, seen: Arc<AtomicI64>, shown: Arc<AtomicI64>) {
    let mut backoff = Duration::from_secs(1);
    let mut last_read: Option<i64> = None;
    let mut last = seen.load(Ordering::SeqCst);
    loop {
        let Some(e) = engine.upgrade() else { return };
        let client = e.client.clone();
        let wake = e.poll_wake.clone();
        drop(e); // don't keep the engine alive across the long poll
        let result = {
            let poll = client.poll_messages(&token, last, POLL_TIMEOUT_S);
            tokio::pin!(poll);
            // The long poll does not see a new reaction before its time is up: Talk answers
            // it from memory (see `TalkClient::newer_messages`). Looking past that memory every
            // few seconds, and whenever something wakes the chats, brings it.
            loop {
                tokio::select! {
                    r = &mut poll => break r,
                    _ = tokio::time::sleep(PEEK_EVERY) => {}
                    _ = wake.notified() => {}
                }
                let peek = client.newer_messages(&token, last, DELTA_PAGE);
                tokio::pin!(peek);
                tokio::select! {
                    r = &mut poll => break r,
                    // What failed here is the long poll's to report.
                    r = &mut peek => if let Ok(batch) = r && !batch.messages.is_empty() {
                        break Ok(batch);
                    },
                }
            }
        };
        let Some(e) = engine.upgrade() else { return };
        match e.guard(result) {
            Ok(batch) => {
                backoff = Duration::from_secs(1);
                let after = last;
                if let Some(id) = batch.last_given.or(batch.messages.last().map(|m| m.id)) {
                    last = last.max(id);
                }
                // No await between here and `apply_live`: a restart never loses or repeats a batch.
                if !e.poll_goes_on(&token, &seen, last) {
                    return;
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
                let refresh = e.apply_live(&token, after, batch.messages);
                shown.store(last, Ordering::SeqCst);
                if refresh {
                    let _ = e.refresh_rooms(true).await;
                }
            }
            Err(Error::Unauthorized) => return,
            // Removed from the conversation, or it was deleted: it leaves the list.
            Err(Error::NotFound) => {
                let _ = e.refresh_rooms(true).await;
                if e.state_room(&token).is_none() {
                    let mut st = e.state.lock().unwrap();
                    if st.chats.get(&token).is_some_and(|poll| Arc::ptr_eq(&poll.last, &seen)) {
                        st.chats.remove(&token);
                    }
                    return;
                }
                drop(e);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
            Err(err) => {
                tracing::warn!(%token, "chat poll failed: {err}");
                drop(e);
                // The network coming back ends the pause.
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => backoff = (backoff * 2).min(Duration::from_secs(30)),
                    _ = wake.notified() => backoff = Duration::from_secs(1),
                }
            }
        }
    }
}

/// The level that applies: Talk's default (0) means always in one-to-one chats (and the
/// note to self) and mentions only in groups.
pub(crate) fn effective_notification_level(level: u8, room_type: RoomType) -> u8 {
    match (level, room_type) {
        (1..=3, _) => level,
        (_, RoomType::OneToOne | RoomType::FormerOneToOne | RoomType::NoteToSelf) => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_notification_level_is_resolved() {
        assert_eq!(effective_notification_level(0, RoomType::OneToOne), 1);
        assert_eq!(effective_notification_level(0, RoomType::NoteToSelf), 1);
        assert_eq!(effective_notification_level(0, RoomType::Group), 2);
        assert_eq!(effective_notification_level(0, RoomType::Public), 2);
        assert_eq!(effective_notification_level(3, RoomType::OneToOne), 3);
        assert_eq!(effective_notification_level(1, RoomType::Group), 1);
    }

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

    fn msg(reactions: &[(&str, i64)], own: &[&str]) -> ChatMessage {
        let mut m: ChatMessage = serde_json::from_str(
            r#"{"id":7,"token":"t","actorType":"users","actorId":"a","actorDisplayName":"A",
                "timestamp":1,"message":"hi"}"#,
        )
        .unwrap();
        m.reactions = reactions.iter().map(|(e, n)| (e.to_string(), *n)).collect();
        m.reactions_self = own.iter().map(|e| e.to_string()).collect();
        m
    }

    #[test]
    fn optimistic_replaces_own_reaction() {
        let m = msg(&[("👍", 2), ("❤️", 1)], &["👍"]);
        let n = optimistic(&m, &m.reactions_self, Some("❤️"));
        assert_eq!(n.reactions.get("👍"), Some(&1));
        assert_eq!(n.reactions.get("❤️"), Some(&2));
        assert_eq!(n.reactions_self, vec!["❤️"]);
    }

    #[test]
    fn optimistic_removes_last_and_drops_empty_pills() {
        let m = msg(&[("👍", 1)], &["👍"]);
        let n = optimistic(&m, &m.reactions_self, None);
        assert!(n.reactions.is_empty());
        assert!(n.reactions_self.is_empty());
        // Several own reactions (set in Talk-Web) all go.
        let m = msg(&[("👍", 1), ("😂", 3)], &["👍", "😂"]);
        let n = optimistic(&m, &m.reactions_self, Some("😮"));
        assert_eq!(n.reactions.len(), 2);
        assert_eq!(n.reactions.get("😂"), Some(&2));
        assert_eq!(n.reactions.get("😮"), Some(&1));
    }

    #[test]
    fn reactions_response_parses() {
        let r: Reactions = serde_json::from_str(
            r#"{"👍":[{"actorType":"users","actorId":"b","actorDisplayName":"Bea","timestamp":5}]}"#,
        )
        .unwrap();
        assert_eq!(r.0["👍"][0].actor_display_name, "Bea");
        let empty: Reactions = serde_json::from_str("[]").unwrap();
        assert!(empty.0.is_empty());
    }

    #[test]
    fn reaction_errors_are_readable() {
        let forbidden = Error::Http { status: 403, message: String::new() };
        assert_eq!(reaction_error_text(&forbidden), "Keine Berechtigung für Reaktionen");
        assert_eq!(reaction_error_text(&Error::Decode("x".into())), "Reaktion fehlgeschlagen");
    }
}
