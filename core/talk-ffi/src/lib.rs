//! Kotlin/Swift facade over `talk-core`, generated with UniFFI.
//!
//! Keep this layer thin: map core types to FFI records and forward calls.

use std::collections::HashMap;
use std::sync::Arc;

use talk_core as core;

uniffi::setup_scaffolding!();

#[uniffi::export]
pub fn core_version() -> String {
    core::VERSION.to_string()
}

#[derive(uniffi::Record)]
pub struct LoginResult {
    pub server: String,
    pub login_name: String,
    pub app_password: String,
}

/// Login Flow v2: open `login_url()` in a browser, then await `wait()`.
#[derive(uniffi::Object)]
pub struct LoginFlow {
    inner: core::LoginFlow,
    cancel: tokio::sync::Notify,
}

#[uniffi::export(async_runtime = "tokio")]
impl LoginFlow {
    #[uniffi::constructor]
    pub async fn start(server: String, platform: ClientPlatform) -> Result<Arc<Self>, CoreError> {
        let inner = core::LoginFlow::start(&server, platform.into()).await?;
        Ok(Arc::new(Self {
            inner,
            cancel: tokio::sync::Notify::new(),
        }))
    }

    pub fn login_url(&self) -> String {
        self.inner.login_url.clone()
    }

    /// Resolves once the user approved in the browser (fails after 20 minutes,
    /// or with `Cancelled` after `cancel()`).
    pub async fn wait(&self) -> Result<LoginResult, CoreError> {
        let c = tokio::select! {
            r = self.inner.wait() => r?,
            _ = self.cancel.notified() => return Err(CoreError::Cancelled),
        };
        Ok(LoginResult {
            server: c.server,
            login_name: c.login_name,
            app_password: c.app_password,
        })
    }

    /// Stop polling (e.g. the user closed the browser). Safe to call before `wait()`.
    pub fn cancel(&self) {
        self.cancel.notify_one();
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("network: {detail}")]
    Network { detail: String },
    #[error("server: {detail}")]
    Server { detail: String },
    #[error("cancelled")]
    Cancelled,
}

impl From<core::Error> for CoreError {
    fn from(e: core::Error) -> Self {
        match e {
            core::Error::Unauthorized => Self::Unauthorized,
            core::Error::Network(e) => Self::Network {
                detail: e.to_string(),
            },
            other => Self::Server {
                detail: other.to_string(),
            },
        }
    }
}

#[derive(uniffi::Enum)]
pub enum ClientPlatform {
    Android,
    Ios,
    Desktop,
}

impl From<ClientPlatform> for core::Platform {
    fn from(p: ClientPlatform) -> Self {
        match p {
            ClientPlatform::Android => Self::Android,
            ClientPlatform::Ios => Self::Ios,
            ClientPlatform::Desktop => Self::Desktop,
        }
    }
}

// --- UI models (mirrors of talk_core::engine views) --------------------------------

/// A file shared into a conversation. `width`/`height` are 0 when unknown.
#[derive(uniffi::Record, Clone)]
pub struct FileAttachment {
    pub id: String,
    pub name: String,
    pub path: String,
    pub mimetype: String,
    pub size: i64,
    pub width: i64,
    pub height: i64,
    pub preview_available: bool,
    pub link: String,
    pub voice_message: bool,
}

impl From<core::FileAttachment> for FileAttachment {
    fn from(a: core::FileAttachment) -> Self {
        Self {
            id: a.id,
            name: a.name,
            path: a.path,
            mimetype: a.mimetype,
            size: a.size,
            width: a.width,
            height: a.height,
            preview_available: a.preview_available,
            link: a.link,
            voice_message: a.voice_message,
        }
    }
}

#[derive(uniffi::Record)]
pub struct MessagePreview {
    pub author: String,
    /// For file messages only the caption (may be empty).
    pub text: String,
    pub is_own: bool,
    pub is_system: bool,
    pub attachment: Option<FileAttachment>,
}

#[derive(uniffi::Record)]
pub struct RoomView {
    pub token: String,
    pub name: String,
    pub description: String,
    /// 1 one-to-one, 2 group, 3 public, 4 changelog, 5 former 1:1, 6 note to self.
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
    pub avatar_version: String,
}

impl From<core::RoomView> for RoomView {
    fn from(r: core::RoomView) -> Self {
        Self {
            token: r.token,
            name: r.name,
            description: r.description,
            room_type: r.room_type,
            unread_messages: r.unread_messages,
            unread_mention: r.unread_mention,
            last_activity: r.last_activity,
            last_read_message: r.last_read_message,
            last_common_read_message: r.last_common_read_message,
            last_message: r.last_message.map(|m| MessagePreview {
                author: m.author,
                text: m.text,
                is_own: m.is_own,
                is_system: m.is_system,
                attachment: m.attachment.map(Into::into),
            }),
            is_favorite: r.is_favorite,
            is_archived: r.is_archived,
            read_only: r.read_only,
            has_call: r.has_call,
            notification_level: r.notification_level,
            avatar_version: r.avatar_version,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Text,
    System,
    Deleted,
}

#[derive(uniffi::Record)]
pub struct ReplyPreview {
    pub id: i64,
    pub author: String,
    pub text: String,
    pub is_own: bool,
    pub attachment: Option<FileAttachment>,
}

#[derive(uniffi::Record)]
pub struct MessageView {
    pub id: i64,
    pub token: String,
    pub actor_type: String,
    pub actor_id: String,
    pub author: String,
    pub timestamp: i64,
    pub text: String,
    pub kind: MessageKind,
    pub system_message: String,
    pub reference_id: String,
    pub is_own: bool,
    pub edited: bool,
    pub markdown: bool,
    pub reply_to: Option<ReplyPreview>,
    pub reactions: HashMap<String, i64>,
    pub reactions_self: Vec<String>,
    /// For file messages; `text` is then the caption.
    pub attachment: Option<FileAttachment>,
}

impl From<core::MessageView> for MessageView {
    fn from(m: core::MessageView) -> Self {
        Self {
            id: m.id,
            token: m.token,
            actor_type: m.actor_type,
            actor_id: m.actor_id,
            author: m.author,
            timestamp: m.timestamp,
            text: m.text,
            kind: match m.kind {
                "system" => MessageKind::System,
                "deleted" => MessageKind::Deleted,
                _ => MessageKind::Text,
            },
            system_message: m.system_message,
            reference_id: m.reference_id,
            is_own: m.is_own,
            edited: m.edited,
            markdown: m.markdown,
            reply_to: m.reply_to.map(|r| ReplyPreview {
                id: r.id,
                author: r.author,
                text: r.text,
                is_own: r.is_own,
                attachment: r.attachment.map(Into::into),
            }),
            reactions: m.reactions.into_iter().collect(),
            reactions_self: m.reactions_self,
            attachment: m.attachment.map(Into::into),
        }
    }
}

#[derive(uniffi::Record)]
pub struct ChatPage {
    pub messages: Vec<MessageView>,
    pub has_more: bool,
    pub oldest_id: Option<i64>,
    pub last_common_read: Option<i64>,
}

impl From<core::ChatPage> for ChatPage {
    fn from(p: core::ChatPage) -> Self {
        Self {
            messages: p.messages.into_iter().map(Into::into).collect(),
            has_more: p.has_more,
            oldest_id: p.oldest_id,
            last_common_read: p.last_common_read,
        }
    }
}

fn views(v: Vec<core::MessageView>) -> Vec<MessageView> {
    v.into_iter().map(Into::into).collect()
}

#[derive(uniffi::Enum)]
pub enum CoreEvent {
    RoomsChanged { rooms: Vec<RoomView> },
    MessagesAdded { token: String, messages: Vec<MessageView> },
    MessagesUpdated { token: String, messages: Vec<MessageView> },
    ReadStatus { token: String, last_common_read: i64 },
    Connection { online: bool },
    AuthFailed,
}

impl From<core::Event> for CoreEvent {
    fn from(e: core::Event) -> Self {
        match e {
            core::Event::RoomsChanged { rooms } => Self::RoomsChanged {
                rooms: rooms.into_iter().map(Into::into).collect(),
            },
            core::Event::MessagesAdded { token, messages } => Self::MessagesAdded {
                token,
                messages: views(messages),
            },
            core::Event::MessagesUpdated { token, messages } => Self::MessagesUpdated {
                token,
                messages: views(messages),
            },
            core::Event::ReadStatus {
                token,
                last_common_read,
            } => Self::ReadStatus {
                token,
                last_common_read,
            },
            core::Event::Connection { online } => Self::Connection { online },
            core::Event::AuthFailed => Self::AuthFailed,
        }
    }
}

/// Implemented on the platform side (Kotlin/Swift) to receive core events.
/// Called from a background thread: hop to the main thread before touching UI.
#[uniffi::export(with_foreign)]
pub trait CoreListener: Send + Sync {
    fn on_event(&self, event: CoreEvent);
}

struct ListenerSink(Arc<dyn CoreListener>);

impl core::EventSink for ListenerSink {
    fn emit(&self, event: core::Event) {
        self.0.on_event(event.into());
    }
}

/// Random id for optimistic messages; the server echo carries the same id.
#[uniffi::export]
pub fn new_reference_id() -> String {
    core::new_reference_id()
}

/// One signed-in account: sync engine plus actions.
#[derive(uniffi::Object)]
pub struct TalkSession {
    engine: Arc<core::Engine>,
}

#[uniffi::export(async_runtime = "tokio")]
impl TalkSession {
    #[uniffi::constructor]
    pub fn new(
        server: String,
        user: String,
        app_password: String,
        platform: ClientPlatform,
        listener: Arc<dyn CoreListener>,
    ) -> Result<Arc<Self>, CoreError> {
        let account = core::Account {
            server,
            user,
            app_password,
        };
        let client = core::TalkClient::new(account, platform.into())?;
        let engine = core::Engine::new(client, Arc::new(ListenerSink(listener)));
        Ok(Arc::new(Self { engine }))
    }

    /// Resolves the user, loads conversations and starts background sync.
    pub async fn start(&self) -> Result<(), CoreError> {
        Ok(self.engine.start().await?)
    }

    pub fn shutdown(&self) {
        self.engine.shutdown();
    }

    pub fn user_id(&self) -> String {
        self.engine.user_id()
    }

    /// Current conversation list (sorted).
    pub fn rooms(&self) -> Vec<RoomView> {
        self.engine.rooms().into_iter().map(Into::into).collect()
    }

    pub async fn refresh_rooms(&self) -> Result<(), CoreError> {
        Ok(self.engine.refresh_rooms(true).await?)
    }

    /// Newest page of a chat; live updates follow as `MessagesAdded` events.
    pub async fn open_chat(&self, token: String) -> Result<ChatPage, CoreError> {
        Ok(self.engine.open_chat(&token).await?.into())
    }

    pub fn close_chat(&self, token: String) {
        self.engine.close_chat(&token);
    }

    pub async fn load_older(&self, token: String, before_id: i64) -> Result<ChatPage, CoreError> {
        Ok(self.engine.load_older(&token, before_id).await?.into())
    }

    pub async fn send(
        &self,
        token: String,
        text: String,
        reference_id: String,
        reply_to: Option<i64>,
    ) -> Result<MessageView, CoreError> {
        Ok(self
            .engine
            .send(&token, &text, &reference_id, reply_to)
            .await?
            .into())
    }

    pub async fn mark_read(&self, token: String, message_id: i64) -> Result<(), CoreError> {
        Ok(self.engine.mark_read(&token, message_id).await?)
    }

    /// Revokes the app password on the server and stops sync.
    pub async fn logout(&self) -> Result<(), CoreError> {
        Ok(self.engine.logout().await?)
    }

    /// Uploads `local_path` to the Talk attachment folder and posts it to the chat.
    /// The message arrives as `MessagesAdded` carrying `reference_id`.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_file(
        &self,
        token: String,
        local_path: String,
        name: String,
        mime: String,
        caption: Option<String>,
        reply_to: Option<i64>,
        reference_id: String,
    ) -> Result<(), CoreError> {
        Ok(self
            .engine
            .send_file(
                &token,
                &local_path,
                &name,
                &mime,
                caption.as_deref(),
                reply_to,
                &reference_id,
            )
            .await?)
    }

    /// Preview image (JPEG/PNG bytes) of a shared file, fitting into `width`×`height`.
    pub async fn file_preview(
        &self,
        file_id: String,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, CoreError> {
        Ok(self.engine.file_preview(&file_id, width, height).await?)
    }

    /// Conversation avatar image (PNG/SVG/JPEG bytes).
    pub async fn avatar(&self, token: String, dark: bool) -> Result<Vec<u8>, CoreError> {
        Ok(self.engine.avatar(&token, dark).await?.0)
    }
}
