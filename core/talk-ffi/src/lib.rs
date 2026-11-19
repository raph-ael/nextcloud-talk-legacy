//! Kotlin/Swift facade over `talk-core`, generated with UniFFI.
//!
//! Keep this layer thin: map core types to FFI records and forward calls.

use std::collections::HashMap;
use std::sync::Arc;

use talk_core as core;

uniffi::setup_scaffolding!();

mod call;
mod log;
mod push;
pub use call::*;
pub use log::*;
pub use push::*;

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

/// Who a fresh login belongs to (docs/accounts.md).
#[derive(uniffi::Record)]
pub struct AccountIdentity {
    pub user_id: String,
    pub display_name: String,
    /// The same for the same user on the same server, whatever the login name was: equal
    /// to the key of an account in the list means "already signed in".
    pub key: String,
}

/// Asks the server who signed in, before the account is stored and started.
#[uniffi::export(async_runtime = "tokio")]
pub async fn resolve_account(
    server: String,
    login_name: String,
    app_password: String,
    platform: ClientPlatform,
) -> Result<AccountIdentity, CoreError> {
    let a = core::resolve_account(&server, &login_name, &app_password, platform.into()).await?;
    Ok(AccountIdentity { user_id: a.user_id, display_name: a.display_name, key: a.key })
}

/// Hands an app password back without a session, e.g. after a login that turned out to
/// be an account which is signed in already.
#[uniffi::export(async_runtime = "tokio")]
pub async fn revoke_app_password(
    server: String,
    login_name: String,
    app_password: String,
    platform: ClientPlatform,
) -> Result<(), CoreError> {
    Ok(core::revoke_app_password(&server, &login_name, &app_password, platform.into()).await?)
}

/// What an account adds to badges (docs/settings.md, docs/accounts.md).
#[derive(uniffi::Record)]
pub struct BadgeView {
    /// Conversations with something new that counts.
    pub chats: u32,
    /// Unread messages in those conversations.
    pub messages: u32,
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
    /// The person of a shared contact (a vCard file); empty for every other file.
    pub contact_name: String,
}

/// A phone number or an e-mail address. `kind`: "mobile", "home", "work" or empty.
#[derive(uniffi::Record, Clone)]
pub struct ContactValue {
    pub kind: String,
    pub value: String,
}

/// A postal address. `kind`: "home", "work" or empty.
#[derive(uniffi::Record, Clone)]
pub struct ContactAddress {
    pub kind: String,
    pub street: String,
    pub city: String,
    pub region: String,
    pub postal_code: String,
    pub country: String,
}

/// The part of an address book entry that is sent (docs/attachments.md "Kontakt").
#[derive(uniffi::Record, Clone)]
pub struct ContactCard {
    /// The name as the address book shows it.
    pub name: String,
    pub given_name: String,
    pub family_name: String,
    pub organization: String,
    pub phones: Vec<ContactValue>,
    pub emails: Vec<ContactValue>,
    pub addresses: Vec<ContactAddress>,
}

impl From<ContactCard> for core::ContactCard {
    fn from(c: ContactCard) -> Self {
        let value = |v: ContactValue| core::ContactValue { kind: v.kind, value: v.value };
        Self {
            name: c.name,
            given_name: c.given_name,
            family_name: c.family_name,
            organization: c.organization,
            phones: c.phones.into_iter().map(value).collect(),
            emails: c.emails.into_iter().map(value).collect(),
            addresses: c
                .addresses
                .into_iter()
                .map(|a| core::ContactAddress {
                    kind: a.kind,
                    street: a.street,
                    city: a.city,
                    region: a.region,
                    postal_code: a.postal_code,
                    country: a.country,
                })
                .collect(),
        }
    }
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
            contact_name: a.contact_name,
        }
    }
}

/// A place shared into a conversation (WGS 84, degrees). `name` may be empty.
#[derive(uniffi::Record, Clone)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub name: String,
}

impl From<core::GeoLocation> for GeoLocation {
    fn from(l: core::GeoLocation) -> Self {
        Self { latitude: l.latitude, longitude: l.longitude, name: l.name }
    }
}

impl From<GeoLocation> for core::GeoLocation {
    fn from(l: GeoLocation) -> Self {
        Self { latitude: l.latitude, longitude: l.longitude, name: l.name }
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
    /// For location messages; `text` is then its name.
    pub location: Option<GeoLocation>,
}

impl From<core::engine::MessagePreview> for MessagePreview {
    fn from(m: core::engine::MessagePreview) -> Self {
        Self {
            author: m.author,
            text: m.text,
            is_own: m.is_own,
            is_system: m.is_system,
            attachment: m.attachment.map(Into::into),
            location: m.location.map(Into::into),
        }
    }
}

/// A conversation with a new message that deserves a notification (docs/notifications.md).
#[derive(uniffi::Record)]
pub struct MessageNotice {
    pub token: String,
    pub room_name: String,
    pub is_group: bool,
    /// The user was mentioned.
    pub mention: bool,
    pub unread: i64,
    /// The newest message as in the conversation list; `None` when the server shows
    /// none (sensitive conversations): "Neue Nachricht".
    pub message: Option<MessagePreview>,
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
    /// The user may react to messages (docs/chat.md „Reaktionen“).
    pub can_react: bool,
    /// The user may write here, and so edit and delete own messages (docs/chat.md
    /// „Bearbeiten und Löschen“).
    pub can_chat: bool,
    /// The user may delete messages of others (moderator of a group).
    pub can_delete_others: bool,
    pub has_call: bool,
    /// 1 always, 2 mentions only, 3 never (the server default is resolved: 1 in
    /// one-to-one chats, 2 in groups).
    pub notification_level: u8,
    pub avatar_version: String,
    /// A picture was set (it can be removed). docs/groups.md
    pub has_custom_avatar: bool,
    /// Owner or moderator: may edit the group and its members.
    pub is_moderator: bool,
    /// `false` when we are the last moderator: promote somebody first.
    pub can_leave: bool,
    pub can_delete: bool,
    /// Group link on (room type 3).
    pub is_public: bool,
    /// Our categories it is in (docs/categories.md); the list uses `room_sections`.
    pub tag_ids: Vec<String>,
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
            last_message: r.last_message.map(Into::into),
            is_favorite: r.is_favorite,
            is_archived: r.is_archived,
            read_only: r.read_only,
            can_react: r.can_react,
            can_chat: r.can_chat,
            can_delete_others: r.can_delete_others,
            has_call: r.has_call,
            notification_level: r.notification_level,
            avatar_version: r.avatar_version,
            has_custom_avatar: r.has_custom_avatar,
            is_moderator: r.is_moderator,
            can_leave: r.can_leave,
            can_delete: r.can_delete,
            is_public: r.is_public,
            tag_ids: r.tag_ids,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Text,
    System,
    Deleted,
}

/// One person's reaction, for the "who reacted" list.
#[derive(uniffi::Record, Clone)]
pub struct ReactionView {
    pub emoji: String,
    pub actor_type: String,
    pub actor_id: String,
    pub name: String,
    pub is_own: bool,
    pub timestamp: i64,
}

impl From<core::ReactionView> for ReactionView {
    fn from(r: core::ReactionView) -> Self {
        Self {
            emoji: r.emoji,
            actor_type: r.actor_type,
            actor_id: r.actor_id,
            name: r.name,
            is_own: r.is_own,
            timestamp: r.timestamp,
        }
    }
}

/// A member of a group (docs/groups.md).
#[derive(uniffi::Record, Clone)]
pub struct MemberView {
    pub attendee_id: i64,
    pub actor_type: String,
    pub actor_id: String,
    pub name: String,
    pub is_self: bool,
    pub is_moderator: bool,
    pub is_owner: bool,
}

impl From<core::MemberView> for MemberView {
    fn from(m: core::MemberView) -> Self {
        Self {
            attendee_id: m.attendee_id,
            actor_type: m.actor_type,
            actor_id: m.actor_id,
            name: m.name,
            is_self: m.is_self,
            is_moderator: m.is_moderator,
            is_owner: m.is_owner,
        }
    }
}

/// A user from the search ("Neue Nachricht", "Mitglieder hinzufügen").
#[derive(uniffi::Record, Clone)]
pub struct UserView {
    pub id: String,
    pub name: String,
    pub subline: String,
}

impl From<core::UserView> for UserView {
    fn from(u: core::UserView) -> Self {
        Self { id: u.id, name: u.name, subline: u.subline }
    }
}

/// One section of the chat list (docs/categories.md).
#[derive(uniffi::Record)]
pub struct SectionView {
    /// For `toggle_section`; row keys are section id + token (a chat may be in several).
    pub id: String,
    /// `favorites`, `custom`, `other`, `groups` or `direct`; in a search also `archived`.
    pub kind: String,
    /// Empty when it is the only section: then no header.
    pub title: String,
    pub collapsed: bool,
    /// Rows to show: none when collapsed.
    pub rooms: Vec<RoomView>,
    /// All chats in the section, hidden ones included.
    pub total: u32,
    /// For the head while collapsed: unread messages of all its chats; muted ones ("Aus")
    /// do not count.
    pub unread: u32,
    /// Show `unread` in grey like the number of a quiet chat: nothing in it counts for the
    /// app badge.
    pub unread_quiet: bool,
    /// For the head while collapsed: an unread mention in one of its chats.
    pub unread_mention: bool,
    /// For the head while collapsed: a running call in one of its chats.
    pub has_call: bool,
}

impl From<core::SectionView> for SectionView {
    fn from(s: core::SectionView) -> Self {
        Self {
            id: s.id,
            kind: s.kind,
            title: s.title,
            collapsed: s.collapsed,
            rooms: s.rooms.into_iter().map(Into::into).collect(),
            total: s.total,
            unread: s.unread,
            unread_quiet: s.unread_quiet,
            unread_mention: s.unread_mention,
            has_call: s.has_call,
        }
    }
}

/// A category for the chat menu "Kategorie" (docs/settings.md, docs/categories.md).
#[derive(uniffi::Record)]
pub struct CategoryView {
    pub id: String,
    /// `favorites`, `custom` or `other`; the chat menu lists only `custom` ones.
    pub kind: String,
    pub name: String,
    /// Chats in it (not archived).
    pub room_count: u32,
    pub collapsed: bool,
}

impl From<core::CategoryView> for CategoryView {
    fn from(c: core::CategoryView) -> Self {
        Self { id: c.id, kind: c.kind, name: c.name, room_count: c.room_count, collapsed: c.collapsed }
    }
}

/// `create_category`: the new `id`, or a German `error` text.
#[derive(uniffi::Record)]
pub struct CreateCategoryResult {
    pub id: Option<String>,
    pub error: Option<String>,
}

fn cat_err(r: core::Result<()>, op: core::CategoryOp) -> Option<String> {
    r.err().map(|e| core::category_error_text(op, &e))
}

/// Result of opening or creating a conversation: its `token`, or a German `error` text.
#[derive(uniffi::Record, Clone)]
pub struct OpenResult {
    pub token: Option<String>,
    pub error: Option<String>,
}

fn open_result(r: core::Result<String>, op: core::GroupOp) -> OpenResult {
    match r {
        Ok(token) => OpenResult { token: Some(token), error: None },
        Err(e) => OpenResult { token: None, error: Some(core::group_error_text(op, &e)) },
    }
}

fn group_err(r: core::Result<()>, op: core::GroupOp) -> Option<String> {
    r.err().map(|e| core::group_error_text(op, &e))
}

#[derive(uniffi::Record, Clone)]
pub struct ReplyPreview {
    pub id: i64,
    pub author: String,
    pub text: String,
    pub is_own: bool,
    pub attachment: Option<FileAttachment>,
    pub location: Option<GeoLocation>,
}

impl From<ReplyPreview> for core::ReplyPreview {
    fn from(r: ReplyPreview) -> Self {
        Self {
            id: r.id,
            author: r.author,
            text: r.text,
            is_own: r.is_own,
            attachment: r.attachment.map(|a| core::FileAttachment {
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
                contact_name: a.contact_name,
            }),
            location: r.location.map(Into::into),
        }
    }
}

/// What the server connection looks like to the user (docs/connection.md).
#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnectionState {
    Online,
    /// Starting, or a call just failed and the core is retrying: a small spinner.
    Connecting,
    /// The retries failed too: "Keine Verbindung".
    Offline,
}

impl From<core::ConnectionState> for ConnectionState {
    fn from(s: core::ConnectionState) -> Self {
        match s {
            core::ConnectionState::Online => Self::Online,
            core::ConnectionState::Connecting => Self::Connecting,
            core::ConnectionState::Offline => Self::Offline,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PendingState {
    Sending,
    /// Waiting for the network or the server; retried automatically.
    Waiting,
    Failed,
}

/// Why a message could not be delivered.
#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SendError {
    Expired,
    Forbidden,
    NotFound,
    TooLarge,
    QuotaExceeded,
    FileMissing,
    Rejected,
}

impl From<core::SendError> for SendError {
    fn from(e: core::SendError) -> Self {
        match e {
            core::SendError::Expired => Self::Expired,
            core::SendError::Forbidden => Self::Forbidden,
            core::SendError::NotFound => Self::NotFound,
            core::SendError::TooLarge => Self::TooLarge,
            core::SendError::QuotaExceeded => Self::QuotaExceeded,
            core::SendError::FileMissing => Self::FileMissing,
            core::SendError::Rejected => Self::Rejected,
        }
    }
}

/// A message in the outbox: shown as a pending bubble until delivered.
#[derive(uniffi::Record)]
pub struct PendingMessage {
    pub message: MessageView,
    pub state: PendingState,
    /// Upload progress 0..1, negative when not uploading.
    pub progress: f32,
    pub error: Option<SendError>,
    /// Local copy of the attachment.
    pub local_path: Option<String>,
}

impl From<core::PendingMessage> for PendingMessage {
    fn from(p: core::PendingMessage) -> Self {
        Self {
            message: p.message.into(),
            state: match p.state {
                core::PendingState::Sending => PendingState::Sending,
                core::PendingState::Waiting => PendingState::Waiting,
                core::PendingState::Failed => PendingState::Failed,
            },
            progress: p.progress,
            error: p.error.map(Into::into),
            local_path: p.local_path,
        }
    }
}

#[derive(uniffi::Record)]
pub struct OutboxIdle {
    /// Items waiting for the network or the server.
    pub waiting: u32,
    pub failed: u32,
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
    /// (docs/mentions.md).
    pub mentions: Vec<MentionView>,
    /// Somebody else mentions the user here (their name or everybody).
    pub mentions_me: bool,
}

/// A mention the user picked from the suggestions: `@{label}` in the text stands for it.
#[derive(uniffi::Record, Clone)]
pub struct MentionPick {
    /// The display name, without the `@`.
    pub label: String,
    pub mention_id: String,
}

impl From<MentionPick> for core::MentionPick {
    fn from(p: MentionPick) -> Self {
        Self { label: p.label, mention_id: p.mention_id }
    }
}

impl From<core::MentionPick> for MentionPick {
    fn from(p: core::MentionPick) -> Self {
        Self { label: p.label, mention_id: p.mention_id }
    }
}

fn picks(mentions: Vec<MentionPick>) -> Vec<core::MentionPick> {
    mentions.into_iter().map(Into::into).collect()
}

/// A mention in a message; `label` ("@Lena Brandt") is how it stands in the text.
#[derive(uniffi::Record, Clone)]
pub struct MentionView {
    pub label: String,
    /// `user`, `group`, `team`, `guest`, `email`, `federated` or `all`.
    pub kind: String,
    /// The user id for `user`, else the mention id.
    pub id: String,
    /// It means the user of this app: their name, or everybody.
    pub is_self: bool,
}

impl From<core::MentionView> for MentionView {
    fn from(m: core::MentionView) -> Self {
        Self { label: m.label, kind: m.kind.to_string(), id: m.id, is_self: m.is_self }
    }
}

/// One row of the suggestions after an `@` (docs/mentions.md).
#[derive(uniffi::Record, Clone)]
pub struct MentionCandidate {
    pub mention_id: String,
    /// The display name, without the `@`.
    pub label: String,
    /// Like `MentionView.kind`.
    pub kind: String,
    /// For `user`: whose picture the row shows.
    pub user_id: Option<String>,
    /// Second line; empty for people.
    pub subline: String,
}

impl From<core::MentionCandidate> for MentionCandidate {
    fn from(c: core::MentionCandidate) -> Self {
        Self { mention_id: c.mention_id, label: c.label, kind: c.kind.to_string(), user_id: c.user_id, subline: c.subline }
    }
}

/// A message's text for the input field, with its mentions.
#[derive(uniffi::Record)]
pub struct EditDraft {
    pub text: String,
    pub mentions: Vec<MentionPick>,
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
                location: r.location.map(Into::into),
            }),
            reactions: m.reactions.into_iter().collect(),
            reactions_self: m.reactions_self,
            attachment: m.attachment.map(Into::into),
            location: m.location.map(Into::into),
            deletable: m.deletable,
            editable: m.editable,
            edit_until: m.edit_until,
            mentions: m.mentions.into_iter().map(Into::into).collect(),
            mentions_me: m.mentions_me,
        }
    }
}

#[derive(uniffi::Record)]
pub struct ChatPage {
    pub messages: Vec<MessageView>,
    pub has_more: bool,
    pub oldest_id: Option<i64>,
    pub last_common_read: Option<i64>,
    /// Replace the shown messages instead of merging (the cached copy was outdated).
    pub reset: bool,
    /// Newer messages exist above this page: a window into the history
    /// (`messages_around`, `load_newer`; docs/search.md).
    pub has_newer: bool,
    /// Pass to `load_newer`.
    pub newest_id: Option<i64>,
    /// Messages behind the read marker that mention the user, oldest first: the "@"
    /// button (docs/mentions.md). Every page tells what it knows; merge them.
    pub unread_mentions: Vec<i64>,
}

/// A piece of a search snippet; `hit` marks a search word.
#[derive(uniffi::Record)]
pub struct SnippetPart {
    pub text: String,
    pub hit: bool,
}

/// A message that matches the search (docs/search.md).
#[derive(uniffi::Record)]
pub struct SearchHit {
    pub token: String,
    pub message_id: i64,
    pub timestamp: i64,
    pub room_name: String,
    pub actor_type: String,
    pub actor_id: String,
    pub actor_name: String,
    pub is_own: bool,
    /// Show the sender before the snippet ("Du: …", "Jonas: …").
    pub show_actor: bool,
    pub snippet: Vec<SnippetPart>,
}

#[derive(uniffi::Record)]
pub struct SearchResults {
    /// Newest first.
    pub hits: Vec<SearchHit>,
    /// In one chat: all matches, also beyond `hits` ("3 von 17"). In all chats: as many
    /// as `hits`.
    pub total: u32,
    /// There are more matches than `hits`: ask again with a higher limit.
    pub more: bool,
    /// Everything is on the device: no need for `search_messages_everywhere`.
    pub complete: bool,
    /// While they differ, the history is still being fetched.
    pub chats_done: u32,
    pub chats_total: u32,
}

impl From<core::SearchResults> for SearchResults {
    fn from(r: core::SearchResults) -> Self {
        Self {
            hits: r
                .hits
                .into_iter()
                .map(|h| SearchHit {
                    token: h.token,
                    message_id: h.message_id,
                    timestamp: h.timestamp,
                    room_name: h.room_name,
                    actor_type: h.actor_type,
                    actor_id: h.actor_id,
                    actor_name: h.actor_name,
                    is_own: h.is_own,
                    show_actor: h.show_actor,
                    snippet: h.snippet.into_iter().map(|p| SnippetPart { text: p.text, hit: p.hit }).collect(),
                })
                .collect(),
            total: r.total,
            more: r.more,
            complete: r.complete,
            chats_done: r.chats_done,
            chats_total: r.chats_total,
        }
    }
}

impl From<core::ChatPage> for ChatPage {
    fn from(p: core::ChatPage) -> Self {
        Self {
            messages: p.messages.into_iter().map(Into::into).collect(),
            has_more: p.has_more,
            oldest_id: p.oldest_id,
            last_common_read: p.last_common_read,
            reset: p.reset,
            has_newer: p.has_newer,
            newest_id: p.newest_id,
            unread_mentions: p.unread_mentions,
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
    /// docs/connection.md. A session starts as `Connecting`.
    Connection { state: ConnectionState },
    AuthFailed,
    /// Pending messages of a conversation changed (any conversation, open or not).
    OutboxChanged { token: String, items: Vec<PendingMessage> },
    /// The call changed; `None`: no call any more (preceded by one `Ended` state).
    CallChanged { call: Option<CallView> },
    /// An incoming call was missed (not answered, or came in during another call).
    CallMissed { token: String, room_name: String, caller_name: String, is_group: bool },
    /// Finished call recordings waiting for the user: the full current list.
    RecordingsChanged { items: Vec<call::RecordingNotice> },
    /// Members or roles of a group changed: reload `members` if shown (docs/groups.md).
    ParticipantsChanged { token: String },
    /// Conversations with a new message that deserves a notification, after the
    /// `RoomsChanged` that brought it and also for the open conversation (whether to
    /// show it is the platform's call). `since_start`: what came in while the app was
    /// closed. Android uses it only without push, iOS not at all (docs/notifications.md).
    MessageNotices { items: Vec<MessageNotice>, since_start: bool },
    /// The notifications of these conversations are obsolete: read, archived or gone.
    NoticesCleared { tokens: Vec<String> },
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
            core::Event::Connection { state } => Self::Connection { state: state.into() },
            core::Event::AuthFailed => Self::AuthFailed,
            core::Event::OutboxChanged { token, items } => Self::OutboxChanged {
                token,
                items: items.into_iter().map(Into::into).collect(),
            },
            core::Event::CallChanged { call } => Self::CallChanged {
                call: call.map(Into::into),
            },
            core::Event::CallMissed {
                token,
                room_name,
                caller_name,
                is_group,
            } => Self::CallMissed {
                token,
                room_name,
                caller_name,
                is_group,
            },
            core::Event::RecordingsChanged { items } => Self::RecordingsChanged {
                items: items.into_iter().map(Into::into).collect(),
            },
            core::Event::ParticipantsChanged { token } => Self::ParticipantsChanged { token },
            core::Event::MessageNotices { items, since_start } => Self::MessageNotices {
                items: items
                    .into_iter()
                    .map(|n| MessageNotice {
                        token: n.token,
                        room_name: n.room_name,
                        is_group: n.is_group,
                        mention: n.mention,
                        unread: n.unread,
                        message: n.message.map(Into::into),
                    })
                    .collect(),
                since_start,
            },
            core::Event::NoticesCleared { tokens } => Self::NoticesCleared { tokens },
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

// --- voice messages ------------------------------------------------------------------

/// A recorded, not yet sent voice message (one per conversation).
#[derive(uniffi::Record)]
pub struct VoiceDraft {
    pub id: String,
    pub token: String,
    /// MP3 file, playable as is.
    pub path: String,
    pub duration_ms: u64,
    pub size: u64,
    /// 100 levels, 0 (silence) ..= 255.
    pub waveform: Vec<u8>,
    pub created_at: i64,
}

impl From<core::VoiceDraft> for VoiceDraft {
    fn from(d: core::VoiceDraft) -> Self {
        Self {
            id: d.id,
            token: d.token,
            path: d.path,
            duration_ms: d.duration_ms,
            size: d.size,
            waveform: d.waveform,
            created_at: d.created_at,
        }
    }
}

#[derive(uniffi::Record)]
pub struct RecordingStatus {
    pub duration_ms: u64,
    /// Current level 0..1.
    pub level: f32,
    /// One hour reached: call `finish`.
    pub limit_reached: bool,
}

#[derive(uniffi::Record)]
pub struct AudioInfo {
    pub duration_ms: u64,
    pub waveform: Vec<u8>,
}

/// Longest voice message in milliseconds.
#[uniffi::export]
pub fn voice_max_duration_ms() -> u64 {
    core::voice::MAX_DURATION_MS
}

/// Shortest voice message kept (shorter ones are discarded on `finish`).
#[uniffi::export]
pub fn voice_min_duration_ms() -> u64 {
    core::voice::MIN_DURATION_MS
}

/// A recording in progress: written to disk as MP3 with every `push`.
#[derive(uniffi::Object)]
pub struct VoiceRecording {
    inner: core::VoiceRecorder,
}

#[uniffi::export]
impl VoiceRecording {
    /// 16-bit little-endian mono PCM at the rate given to `start_voice`.
    pub fn push(&self, pcm: Vec<u8>) -> Result<RecordingStatus, CoreError> {
        let samples: Vec<i16> = pcm.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        let s = self.inner.push(&samples)?;
        Ok(RecordingStatus {
            duration_ms: s.duration_ms,
            level: s.level,
            limit_reached: s.limit_reached,
        })
    }

    /// Ends the recording as the conversation's draft (`None`: shorter than a second).
    pub fn finish(&self) -> Result<Option<VoiceDraft>, CoreError> {
        Ok(self.inner.finish()?.map(Into::into))
    }

    /// Ends the recording and deletes it.
    pub fn cancel(&self) {
        self.inner.cancel();
    }
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
        data_dir: Option<String>,
    ) -> Result<Arc<Self>, CoreError> {
        let account = core::Account {
            server,
            user,
            app_password,
        };
        let client = core::TalkClient::new(account, platform.into())?;
        let engine = core::Engine::new(
            client,
            Arc::new(ListenerSink(listener)),
            data_dir.map(Into::into),
        );
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

    /// The user's name as the server shows it, for the account list. From the cache at
    /// once, up to date after `start()`; the user id while nothing else is known.
    pub fn display_name(&self) -> String {
        self.engine.display_name()
    }

    /// What this account adds to badges: muted and archived chats do not count, "Nur
    /// Erwähnungen" only with a mention. Read it after every `RoomsChanged`.
    pub fn badge(&self) -> BadgeView {
        let b = self.engine.badge();
        BadgeView { chats: b.chats, messages: b.messages }
    }

    /// Current conversation list (sorted).
    pub fn rooms(&self) -> Vec<RoomView> {
        self.engine.rooms().into_iter().map(Into::into).collect()
    }

    pub async fn refresh_rooms(&self) -> Result<(), CoreError> {
        Ok(self.engine.refresh_rooms(true).await?)
    }

    /// Newest cached messages (empty without cache), for the first frame of a chat.
    pub fn cached_chat(&self, token: String) -> ChatPage {
        self.engine.cached_chat(&token).into()
    }

    /// Brings a chat up to date and returns its newest page; live updates follow as
    /// `MessagesAdded` events.
    pub async fn open_chat(&self, token: String) -> Result<ChatPage, CoreError> {
        Ok(self.engine.open_chat(&token).await?.into())
    }

    pub fn close_chat(&self, token: String) {
        self.engine.close_chat(&token);
    }

    pub async fn load_older(&self, token: String, before_id: i64) -> Result<ChatPage, CoreError> {
        Ok(self.engine.load_older(&token, before_id).await?.into())
    }

    /// A window into the history around a message (search hit, quoted message). Page on
    /// with `load_older` and `load_newer`; `cached_chat` leads back to the end.
    pub async fn messages_around(&self, token: String, message_id: i64) -> Result<ChatPage, CoreError> {
        Ok(self.engine.messages_around(&token, message_id).await?.into())
    }

    /// Messages after `after_id` (`ChatPage.newest_id`) of a window into the history.
    pub async fn load_newer(&self, token: String, after_id: i64) -> Result<ChatPage, CoreError> {
        Ok(self.engine.load_newer(&token, after_id).await?.into())
    }

    /// Searches the messages on the device, in one chat or (`token` null) in all. Answers
    /// within milliseconds, but reads a database: not on the main thread.
    pub fn search_messages(&self, query: String, token: Option<String>, limit: u32) -> SearchResults {
        self.engine.search_messages(&query, token.as_deref(), limit).into()
    }

    /// The same with the server's hits added. Only needed while `complete` is false; call
    /// it when the user paused typing (300 ms).
    pub async fn search_messages_everywhere(
        &self,
        query: String,
        token: Option<String>,
        limit: u32,
    ) -> Result<SearchResults, CoreError> {
        Ok(self.engine.search_messages_everywhere(&query, token.as_deref(), limit).await?.into())
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

    /// Sets the own reaction like Signal (one per person): `emoji` replaces it, the same
    /// emoji again or `None` removes it. Shows at once via `MessagesUpdated`. Returns a
    /// short German error text to show, or `None` on success (docs/chat.md „Reaktionen“).
    pub async fn set_reaction(&self, token: String, message_id: i64, emoji: Option<String>) -> Option<String> {
        self.engine
            .set_reaction(&token, message_id, emoji.as_deref())
            .await
            .err()
            .map(|e| core::reaction_error_text(&e))
    }

    /// The text of a message for the input field when editing it, with its mentions
    /// (hand them back to `edit_message`). `None`: start from `MessageView.text`.
    pub async fn edit_source(&self, token: String, message_id: i64) -> Option<EditDraft> {
        let draft = self.engine.edit_source(&token, message_id).await?;
        Some(EditDraft { text: draft.text, mentions: draft.mentions.into_iter().map(Into::into).collect() })
    }

    /// Who can be mentioned in a chat, for what was typed after the `@` (empty:
    /// everybody). The first call of a chat asks the server, later ones answer from
    /// memory. Empty in a one-to-one chat and without a match (docs/mentions.md).
    pub async fn mention_candidates(&self, token: String, query: String, limit: u32) -> Vec<MentionCandidate> {
        self.engine.mention_candidates(&token, &query, limit).await.into_iter().map(Into::into).collect()
    }

    /// The one-to-one chat with a person when there is one already; `open_direct`
    /// creates it.
    pub fn existing_direct(&self, user_id: String) -> Option<String> {
        self.engine.existing_direct(&user_id)
    }

    /// Replaces the text of an own message, or the caption of an own file. The new version
    /// arrives as `MessagesUpdated` before this returns. Returns a short German error
    /// text to show, or `None` on success (docs/chat.md „Bearbeiten und Löschen“).
    /// `mentions`: what `edit_source` gave and what the user picked since.
    pub async fn edit_message(
        &self,
        token: String,
        message_id: i64,
        text: String,
        mentions: Vec<MentionPick>,
    ) -> Option<String> {
        self.engine
            .edit_message(&token, message_id, &text, &picks(mentions))
            .await
            .err()
            .map(|e| core::message_error_text(core::MessageOp::Edit, &e))
    }

    /// Deletes a message for everybody. Returns a short German error text to show, or
    /// `None` on success.
    pub async fn delete_message(&self, token: String, message_id: i64) -> Option<String> {
        self.engine
            .delete_message(&token, message_id)
            .await
            .err()
            .map(|e| core::message_error_text(core::MessageOp::Delete, &e))
    }

    /// Who reacted to a message: own reactions first, then newest first.
    pub async fn reactions(&self, token: String, message_id: i64) -> Result<Vec<ReactionView>, CoreError> {
        Ok(self
            .engine
            .reactions(&token, message_id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    // --- categories (docs/categories.md) ---------------------------------------------

    /// The chat list in sections ("Favoriten", categories, "Chats"). Call again on
    /// `RoomsChanged`, when the search text or `split` changes. `split` is the device
    /// setting "Gruppen und Direktchats trennen": "Gruppen" and "Direktchats" instead of
    /// "Chats" (ids and kinds `groups`, `direct`). `active_token` is no longer used.
    #[uniffi::method(default(split = false))]
    pub fn room_sections(&self, query: String, active_token: Option<String>, split: bool) -> Vec<SectionView> {
        self.engine.room_sections(&query, active_token.as_deref(), split).into_iter().map(Into::into).collect()
    }

    /// Opens or closes a section (saved on the server). German error text or `None`;
    /// the list updates via `RoomsChanged` either way.
    pub async fn toggle_section(&self, id: String) -> Option<String> {
        self.engine
            .toggle_section(&id)
            .await
            .err()
            .map(|e| core::category_error_text(core::CategoryOp::Collapse, &e))
    }

    /// "Sortierung" (docs/categories.md): `activity` or `alphabetical`, for every section.
    pub fn sort_order(&self) -> String {
        self.engine.sort_order()
    }

    /// Chooses the sort order (saved on the server where it knows the setting). German
    /// error text or `None`; the list updates via `RoomsChanged` either way.
    pub async fn set_sort_order(&self, order: String) -> Option<String> {
        cat_err(self.engine.set_sort_order(&order).await, core::CategoryOp::Sort)
    }

    /// The server keeps categories: offer "Kategorie" in the chat menu.
    pub fn categories_supported(&self) -> bool {
        self.engine.categories_supported()
    }

    /// All categories in order, built-in "Favoriten" and "Chats" included.
    pub fn categories(&self) -> Vec<CategoryView> {
        self.engine.categories().into_iter().map(Into::into).collect()
    }

    /// "Neue Kategorie …" from the chat menu: created with these chats in it.
    pub async fn create_category(&self, name: String, tokens: Vec<String>) -> CreateCategoryResult {
        match self.engine.create_category(&name, &tokens).await {
            Ok(id) => CreateCategoryResult { id: Some(id), error: None },
            Err(e) => CreateCategoryResult {
                id: None,
                error: Some(core::category_error_text(core::CategoryOp::Create, &e)),
            },
        }
    }

    // --- chat menu (docs/settings.md); changes return a German error text or `None` ---

    /// The chat is in exactly these own categories afterwards.
    pub async fn set_room_categories(&self, token: String, ids: Vec<String>) -> Option<String> {
        cat_err(self.engine.set_room_categories(&token, &ids).await, core::CategoryOp::Assign)
    }

    /// Shows at once (`RoomsChanged`); taken back on failure.
    pub async fn set_favorite(&self, token: String, on: bool) -> Option<String> {
        cat_err(self.engine.set_favorite(&token, on).await, core::CategoryOp::Favorite)
    }

    /// "Als gelesen markieren": read marker on the newest message.
    pub async fn mark_room_read(&self, token: String) -> Option<String> {
        cat_err(self.engine.mark_room_read(&token).await, core::CategoryOp::MarkRead)
    }

    // --- groups (docs/groups.md); changes return a German error text or `None` -------

    /// Whether "Neue Gruppe" is offered.
    pub fn can_create_groups(&self) -> bool {
        self.engine.can_create_groups()
    }

    /// Users for "Neue Nachricht" (all listable ones for an empty `query`); with
    /// `exclude_token` without the members of that group ("Mitglieder hinzufügen").
    pub async fn search_users(&self, query: String, exclude_token: Option<String>) -> Result<Vec<UserView>, CoreError> {
        Ok(self.engine.search_users(&query, exclude_token.as_deref()).await?.into_iter().map(Into::into).collect())
    }

    /// The one-to-one chat with a user (created when missing).
    pub async fn open_direct(&self, user_id: String) -> OpenResult {
        open_result(self.engine.open_direct(&user_id).await, core::GroupOp::OpenDirect)
    }

    /// Creates a group; `photo`: any image bytes (cropped to a square by the core).
    pub async fn create_group(&self, name: String, user_ids: Vec<String>, photo: Option<Vec<u8>>) -> OpenResult {
        open_result(self.engine.create_group(&name, &user_ids, photo).await, core::GroupOp::Create)
    }

    /// Members: ourselves first, then moderators, then alphabetically.
    pub async fn members(&self, token: String) -> Result<Vec<MemberView>, CoreError> {
        Ok(self.engine.members(&token).await?.into_iter().map(Into::into).collect())
    }

    pub async fn add_members(&self, token: String, user_ids: Vec<String>) -> Option<String> {
        group_err(self.engine.add_members(&token, &user_ids).await, core::GroupOp::AddMembers)
    }

    pub async fn remove_member(&self, token: String, attendee_id: i64) -> Option<String> {
        group_err(self.engine.remove_member(&token, attendee_id).await, core::GroupOp::RemoveMember)
    }

    pub async fn set_moderator(&self, token: String, attendee_id: i64, moderator: bool) -> Option<String> {
        group_err(self.engine.set_moderator(&token, attendee_id, moderator).await, core::GroupOp::SetModerator)
    }

    pub async fn rename_group(&self, token: String, name: String) -> Option<String> {
        group_err(self.engine.rename_group(&token, &name).await, core::GroupOp::Rename)
    }

    pub async fn set_description(&self, token: String, description: String) -> Option<String> {
        group_err(self.engine.set_description(&token, &description).await, core::GroupOp::Describe)
    }

    /// New group picture (any image bytes), or `None` to remove it.
    pub async fn set_group_photo(&self, token: String, photo: Option<Vec<u8>>) -> Option<String> {
        group_err(self.engine.set_group_photo(&token, photo).await, core::GroupOp::Photo)
    }

    pub async fn set_group_link(&self, token: String, on: bool) -> Option<String> {
        group_err(self.engine.set_group_link(&token, on).await, core::GroupOp::Link)
    }

    pub fn group_link(&self, token: String) -> String {
        self.engine.group_link(&token)
    }

    /// 1 all messages, 2 mentions only, 3 off (chat menu and group settings).
    pub async fn set_notification_level(&self, token: String, level: u8) -> Option<String> {
        group_err(self.engine.set_notification_level(&token, level).await, core::GroupOp::Mute)
    }

    /// Ask when "Gruppe verlassen" is tapped: `true` → "Neuen Moderator wählen" first.
    pub async fn leave_needs_moderator(&self, token: String) -> Result<bool, CoreError> {
        Ok(self.engine.leave_needs_moderator(&token).await?)
    }

    pub async fn leave_group(&self, token: String) -> Option<String> {
        group_err(self.engine.leave_group(&token).await, core::GroupOp::Leave)
    }

    pub async fn delete_group(&self, token: String) -> Option<String> {
        group_err(self.engine.delete_group(&token).await, core::GroupOp::Delete)
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

    // --- outbox: reliable sending ---------------------------------------------------

    /// Queues a text message; delivered in the background (retried, survives restarts).
    /// The pending bubble also arrives as `OutboxChanged`. `mentions`: what the user
    /// picked from the suggestions (docs/mentions.md).
    pub fn enqueue_text(
        &self,
        token: String,
        text: String,
        mentions: Vec<MentionPick>,
        reply_to: Option<ReplyPreview>,
        reference_id: String,
    ) -> PendingMessage {
        self.engine
            .enqueue_text(&token, &text, picks(mentions), reply_to.map(Into::into), &reference_id)
            .into()
    }

    /// Queues a file. `local_path` is moved into the outbox; don't use it afterwards.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_file(
        &self,
        token: String,
        local_path: String,
        name: String,
        mime: String,
        width: i64,
        height: i64,
        caption: String,
        mentions: Vec<MentionPick>,
        reply_to: Option<ReplyPreview>,
        reference_id: String,
    ) -> Result<PendingMessage, CoreError> {
        Ok(self
            .engine
            .enqueue_file(
                &token,
                &local_path,
                &name,
                &mime,
                width,
                height,
                &caption,
                picks(mentions),
                reply_to.map(Into::into),
                &reference_id,
            )?
            .into())
    }

    /// Queues a contact. It goes out as a vCard file named after the person and runs
    /// like any file: progress, "Wartet auf Netz", retry.
    pub fn enqueue_contact(
        &self,
        token: String,
        card: ContactCard,
        reply_to: Option<ReplyPreview>,
        reference_id: String,
    ) -> Result<PendingMessage, CoreError> {
        Ok(self
            .engine
            .enqueue_contact(&token, &card.into(), reply_to.map(Into::into), &reference_id)?
            .into())
    }

    /// Queues a place (WGS 84, degrees); `name` is what the chat shows for it, usually
    /// the address. It waits for the network like a text. Talk has no quote for places.
    pub fn enqueue_location(
        &self,
        token: String,
        latitude: f64,
        longitude: f64,
        name: String,
        reference_id: String,
    ) -> PendingMessage {
        self.engine
            .enqueue_location(&token, latitude, longitude, &name, &reference_id)
            .into()
    }

    /// Pending and failed messages of a conversation, in send order.
    pub fn pending(&self, token: String) -> Vec<PendingMessage> {
        self.engine.pending(&token).into_iter().map(Into::into).collect()
    }

    /// Sends a failed message again, or retries a waiting one right now.
    pub fn retry_send(&self, reference_id: String) {
        self.engine.retry_send(&reference_id);
    }

    /// Drops a pending or failed message.
    pub fn discard(&self, reference_id: String) {
        self.engine.discard(&reference_id);
    }

    /// Call when connectivity returns or changes and when the app comes to the front:
    /// waiting messages are retried at once, the chat list is fetched.
    pub fn network_changed(&self) {
        self.engine.network_changed();
    }

    /// Call when the device has no network at all (docs/connection.md): the state turns
    /// `connecting` now, not after a request that hangs.
    pub fn network_lost(&self) {
        self.engine.network_lost();
    }

    /// The connection state right now (the event only reports changes).
    pub fn connection_state(&self) -> ConnectionState {
        self.engine.connection_state().into()
    }

    /// Number of messages still to deliver (failed ones excluded).
    pub fn outbox_unsent(&self) -> u32 {
        self.engine.outbox_unsent()
    }

    /// Bytes of attachments still to upload.
    pub fn outbox_upload_bytes(&self) -> u64 {
        self.engine.outbox_upload_bytes()
    }

    /// Resolves once nothing is actively being sent (empty, or only waiting/failed items).
    pub async fn wait_outbox_idle(&self) -> OutboxIdle {
        let idle = self.engine.wait_outbox_idle().await;
        OutboxIdle {
            waiting: idle.waiting,
            failed: idle.failed,
        }
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

    // --- voice messages ---------------------------------------------------------------

    /// Starts recording a voice message (16-bit mono PCM at `sample_rate` Hz).
    pub fn start_voice(&self, token: String, sample_rate: u32) -> Result<Arc<VoiceRecording>, CoreError> {
        Ok(Arc::new(VoiceRecording {
            inner: self.engine.start_voice(&token, sample_rate)?,
        }))
    }

    /// The unsent recording of a conversation (recovers an interrupted one; off the UI thread).
    pub fn voice_draft(&self, token: String) -> Option<VoiceDraft> {
        self.engine.voice_draft(&token).map(Into::into)
    }

    /// Conversations with an unsent recording.
    pub fn voice_draft_tokens(&self) -> Vec<String> {
        self.engine.voice_draft_tokens()
    }

    pub fn discard_voice_draft(&self, token: String) {
        self.engine.discard_voice_draft(&token);
    }

    /// Sends the draft through the outbox as a voice message named `name` (.mp3).
    pub fn send_voice_draft(
        &self,
        token: String,
        name: String,
        reply_to: Option<ReplyPreview>,
        reference_id: String,
    ) -> Result<PendingMessage, CoreError> {
        Ok(self
            .engine
            .send_voice_draft(&token, &name, reply_to.map(Into::into), &reference_id)?
            .into())
    }

    /// Local copy of a shared file (downloaded once, resumable, kept for offline use).
    pub async fn fetch_file(&self, path: String, size: i64) -> Result<String, CoreError> {
        Ok(self.engine.fetch_file(&path, size).await?)
    }

    /// Duration and waveform of a local audio file (decoded once, cached).
    pub async fn audio_info(&self, local_path: String) -> Result<AudioInfo, CoreError> {
        let i = self.engine.audio_info(&local_path).await?;
        Ok(AudioInfo {
            duration_ms: i.duration_ms,
            waveform: i.waveform,
        })
    }

    /// Conversation avatar image (PNG/SVG/JPEG bytes).
    pub async fn avatar(&self, token: String, dark: bool) -> Result<Vec<u8>, CoreError> {
        Ok(self.engine.avatar(&token, dark).await?.0)
    }
}
