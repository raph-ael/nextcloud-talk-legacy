//! Node-API facade over `talk-core` for the Electron main process.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use talk_core as core;

fn to_napi(e: core::Error) -> Error {
    let status = match e {
        core::Error::Unauthorized => "UNAUTHORIZED",
        core::Error::Network(_) => "NETWORK",
        _ => "SERVER",
    };
    Error::new(Status::GenericFailure, format!("{status}: {e}"))
}

#[napi]
pub fn core_version() -> String {
    core::VERSION.to_string()
}

#[napi(object)]
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

#[napi]
pub struct TalkSession {
    client: core::TalkClient,
}

#[napi]
impl TalkSession {
    #[napi(constructor)]
    pub fn new(server: String, user: String, app_password: String) -> Result<Self> {
        let account = core::Account {
            server,
            user,
            app_password,
        };
        let client = core::TalkClient::new(account, core::Platform::Desktop).map_err(to_napi)?;
        Ok(Self { client })
    }

    #[napi]
    pub async fn rooms(&self) -> Result<Vec<RoomSummary>> {
        let mut batch = self.client.rooms(None, false).await.map_err(to_napi)?;
        batch
            .rooms
            .sort_by_key(|r| std::cmp::Reverse(r.last_activity));
        Ok(batch
            .rooms
            .iter()
            .map(|r| RoomSummary {
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
            })
            .collect())
    }

    /// Returns the server-assigned message id.
    #[napi]
    pub async fn send_message(&self, token: String, text: String) -> Result<i64> {
        let msg = self
            .client
            .send_message(&token, &text, &core::new_reference_id(), None)
            .await
            .map_err(to_napi)?;
        Ok(msg.id)
    }
}
