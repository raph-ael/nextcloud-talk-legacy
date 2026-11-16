//! Shared core of the Wolkenfunk clients (Android, iOS, Desktop).
//!
//! Layering:
//! - [`client`]: stateless HTTP access to the Nextcloud/Talk OCS APIs (one per account)
//! - [`login`]: Login Flow v2
//! - [`models`]: wire types
//! - [`engine`]: sync engine emitting UI-ready events (shared by all platforms)
//!
//! - `cache`: SQLite cache of rooms and messages, avatar/preview files
//! - [`voice`]: voice messages (MP3 recording, drafts, downloads, waveforms)
//! - [`call`]: audio calls over the HPB (signaling, call state machine; media via [`MediaEngine`])
//!
//! - [`push`]: push notifications (device key, registration, decryption)
//! - [`accounts`]: several accounts side by side (identity, badge)
//! - [`notices`]: which conversations deserve a notification

pub mod accounts;
pub mod call;
pub mod categories;
pub(crate) mod cache;
pub mod client;
pub mod connection;
pub mod engine;
pub mod error;
pub mod groups;
pub mod login;
pub mod models;
pub mod notices;
pub mod outbox;
pub mod push;
pub mod voice;

pub use accounts::{AccountIdentity, BadgeView, account_key, resolve_account, revoke_app_password, server_key};
pub use call::{
    CallDirection, CallParticipant, CallRecording, CallScreen, CallState, CallView, EndReason, IceCandidate, MediaEngine,
    RecordingNotice, RecordingNoticeKind,
    MediaReport, PeerKind, PeerState, SdpType,
};
pub use client::{
    Account, ChatBatch, CurrentUser, Platform, RoomsBatch, TalkClient, new_reference_id,
};
pub use connection::ConnectionState;
pub use engine::{
    ChatPage, Engine, Event, EventSink, MessageOp, MessageView, ReactionView, ReplyPreview, RoomView, message_error_text,
    reaction_error_text,
};
pub use categories::{CategoryOp, CategoryView, SectionView, category_error_text};
pub use error::{Error, Result};
pub use groups::{GroupOp, MemberView, UserView, group_error_text, square_photo};
pub use notices::MessageNotice;
pub use push::{ChatPushParts, PushMessage, PushRegistration};
pub use outbox::{OutboxIdle, PendingMessage, PendingState, SendError};
pub use voice::{AudioInfo, RecordingStatus, VoiceDraft, VoiceRecorder};
pub use login::{LoginFlow, normalize_server};
pub use models::{ChatMessage, ConversationTag, FileAttachment, IceServer, LoginCredentials, Participant, Reaction, Reactions, Room, RoomType, SignalingSettings};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
