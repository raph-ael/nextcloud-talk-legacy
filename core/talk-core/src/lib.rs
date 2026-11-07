//! Shared core of the Sirplus Talk clients (Android, iOS, Desktop).
//!
//! Layering:
//! - [`client`]: stateless HTTP access to the Nextcloud/Talk OCS APIs (one per account)
//! - [`login`]: Login Flow v2
//! - [`models`]: wire types
//!
//! Upcoming: local database + sync engine, signaling client (HPB), call state machine, push.

pub mod client;
pub mod error;
pub mod login;
pub mod models;

pub use client::{Account, ChatBatch, Platform, RoomsBatch, TalkClient, new_reference_id};
pub use error::{Error, Result};
pub use login::{LoginFlow, normalize_server};
pub use models::{ChatMessage, IceServer, LoginCredentials, Room, RoomType, SignalingSettings};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
