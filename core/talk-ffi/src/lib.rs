//! Kotlin/Swift facade over `talk-core`, generated with UniFFI.
//!
//! Keep this layer thin: map core types to FFI records and forward calls.

use std::sync::{Arc, Mutex};

use talk_core as core;

uniffi::setup_scaffolding!();

#[uniffi::export]
pub fn core_version() -> String {
    core::VERSION.to_string()
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("network: {message}")]
    Network { message: String },
    #[error("server: {message}")]
    Server { message: String },
}

impl From<core::Error> for CoreError {
    fn from(e: core::Error) -> Self {
        match e {
            core::Error::Unauthorized => Self::Unauthorized,
            core::Error::Network(e) => Self::Network {
                message: e.to_string(),
            },
            other => Self::Server {
                message: other.to_string(),
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

#[derive(uniffi::Record)]
pub struct RoomSummary {
    pub token: String,
    pub display_name: String,
    pub room_type: u8,
    pub unread_messages: i64,
    pub unread_mention: bool,
    pub last_activity: i64,
    pub last_message_author: Option<String>,
    pub last_message_preview: Option<String>,
    pub has_call: bool,
}

impl From<&core::Room> for RoomSummary {
    fn from(r: &core::Room) -> Self {
        Self {
            token: r.token.clone(),
            display_name: r.display_name.clone(),
            room_type: r.room_type.into(),
            unread_messages: r.unread_messages,
            unread_mention: r.unread_mention,
            last_activity: r.last_activity,
            last_message_author: r
                .last_message
                .as_ref()
                .map(|m| m.actor_display_name.clone()),
            last_message_preview: r.last_message.as_ref().map(|m| m.rendered_text()),
            has_call: r.has_call,
        }
    }
}

#[derive(uniffi::Enum)]
pub enum CoreEvent {
    RoomsUpdated { count: u32 },
}

/// Implemented on the platform side (Kotlin/Swift) to receive core events.
#[uniffi::export(with_foreign)]
pub trait CoreListener: Send + Sync {
    fn on_event(&self, event: CoreEvent);
}

#[derive(uniffi::Object)]
pub struct TalkSession {
    client: core::TalkClient,
    listener: Mutex<Option<Arc<dyn CoreListener>>>,
}

#[uniffi::export(async_runtime = "tokio")]
impl TalkSession {
    #[uniffi::constructor]
    pub fn new(
        server: String,
        user: String,
        app_password: String,
        platform: ClientPlatform,
    ) -> Result<Arc<Self>, CoreError> {
        let account = core::Account {
            server,
            user,
            app_password,
        };
        Ok(Arc::new(Self {
            client: core::TalkClient::new(account, platform.into())?,
            listener: Mutex::new(None),
        }))
    }

    pub fn set_listener(&self, listener: Arc<dyn CoreListener>) {
        *self.listener.lock().unwrap() = Some(listener);
    }

    pub async fn rooms(&self) -> Result<Vec<RoomSummary>, CoreError> {
        let mut batch = self.client.rooms(None, false).await?;
        batch
            .rooms
            .sort_by_key(|r| std::cmp::Reverse(r.last_activity));
        let rooms: Vec<RoomSummary> = batch.rooms.iter().map(Into::into).collect();
        if let Some(l) = self.listener.lock().unwrap().clone() {
            l.on_event(CoreEvent::RoomsUpdated {
                count: rooms.len() as u32,
            });
        }
        Ok(rooms)
    }

    /// Returns the server-assigned message id.
    pub async fn send_message(&self, token: String, text: String) -> Result<i64, CoreError> {
        let msg = self
            .client
            .send_message(&token, &text, &core::new_reference_id(), None)
            .await?;
        Ok(msg.id)
    }
}
