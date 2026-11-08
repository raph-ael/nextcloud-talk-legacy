//! Shared core of the Sirplus Talk clients (Android, iOS, Desktop).
//!
//! Layering:
//! - [`client`]: stateless HTTP access to the Nextcloud/Talk OCS APIs (one per account)
//! - [`login`]: Login Flow v2
//! - [`models`]: wire types
//! - [`engine`]: sync engine emitting UI-ready events (shared by all platforms)
//!
//! Upcoming: local database + sync engine, signaling client (HPB), call state machine, push.

pub mod client;
pub mod engine;
pub mod error;
pub mod login;
pub mod models;
pub mod outbox;

pub use client::{
    Account, ChatBatch, CurrentUser, Platform, RoomsBatch, TalkClient, new_reference_id,
};
pub use engine::{ChatPage, Engine, Event, EventSink, MessageView, ReplyPreview, RoomView};
pub use error::{Error, Result};
pub use outbox::{OutboxIdle, PendingMessage, PendingState, SendError};
pub use login::{LoginFlow, normalize_server};
pub use models::{ChatMessage, FileAttachment, IceServer, LoginCredentials, Room, RoomType, SignalingSettings};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
