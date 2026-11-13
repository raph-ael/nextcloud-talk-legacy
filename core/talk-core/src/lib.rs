//! Shared core of the Sirplus Talk clients (Android, iOS, Desktop).
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

pub mod call;
pub(crate) mod cache;
pub mod client;
pub mod engine;
pub mod error;
pub mod groups;
pub mod login;
pub mod models;
pub mod outbox;
pub mod push;
pub mod voice;

pub use call::{
    CallDirection, CallParticipant, CallRecording, CallScreen, CallState, CallView, EndReason, IceCandidate, MediaEngine,
    RecordingNotice, RecordingNoticeKind,
    MediaReport, PeerKind, PeerState, SdpType,
};
pub use client::{
    Account, ChatBatch, CurrentUser, Platform, RoomsBatch, TalkClient, new_reference_id,
};
pub use engine::{ChatPage, Engine, Event, EventSink, MessageView, ReactionView, ReplyPreview, RoomView, reaction_error_text};
pub use error::{Error, Result};
pub use groups::{GroupOp, MemberView, UserView, group_error_text, square_photo};
pub use push::{PushMessage, PushRegistration};
pub use outbox::{OutboxIdle, PendingMessage, PendingState, SendError};
pub use voice::{AudioInfo, RecordingStatus, VoiceDraft, VoiceRecorder};
pub use login::{LoginFlow, normalize_server};
pub use models::{ChatMessage, FileAttachment, IceServer, LoginCredentials, Participant, Reaction, Reactions, Room, RoomType, SignalingSettings};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
