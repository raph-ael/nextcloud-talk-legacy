//! Push (Android FCM, iOS PushKit): device key, registration, decryption.

use talk_core as core;

use crate::{CoreError, TalkSession};

#[derive(uniffi::Enum, Clone)]
pub enum PushMessage {
    Call { token: String, notification_id: i64, subject: String },
    Chat { token: String, notification_id: i64, subject: String },
    Delete { notification_ids: Vec<i64> },
    DeleteAll,
    Other { app: String, subject: String },
}

impl From<core::PushMessage> for PushMessage {
    fn from(m: core::PushMessage) -> Self {
        match m {
            core::PushMessage::Call { token, notification_id, subject } => Self::Call { token, notification_id, subject },
            core::PushMessage::Chat { token, notification_id, subject } => Self::Chat { token, notification_id, subject },
            core::PushMessage::Delete { notification_ids } => Self::Delete { notification_ids },
            core::PushMessage::DeleteAll => Self::DeleteAll,
            core::PushMessage::Other { app, subject } => Self::Other { app, subject },
        }
    }
}

impl From<PushMessage> for core::PushMessage {
    fn from(m: PushMessage) -> Self {
        match m {
            PushMessage::Call { token, notification_id, subject } => Self::Call { token, notification_id, subject },
            PushMessage::Chat { token, notification_id, subject } => Self::Chat { token, notification_id, subject },
            PushMessage::Delete { notification_ids } => Self::Delete { notification_ids },
            PushMessage::DeleteAll => Self::DeleteAll,
            PushMessage::Other { app, subject } => Self::Other { app, subject },
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct PushRegistration {
    pub device_identifier: String,
    pub user_public_key: String,
    pub proxy: String,
}

/// New device key (PKCS#8 PEM). Generate once per account and keep it in the keystore.
#[uniffi::export]
pub fn generate_push_key() -> Result<String, CoreError> {
    Ok(core::push::generate_key()?)
}

/// Decrypts a push `subject` (base64, from the FCM data / VoIP payload). Synchronous, no
/// network, works before any session exists (iOS PushKit cold start).
#[uniffi::export]
pub fn decrypt_push(device_key_pem: String, subject: String) -> Result<PushMessage, CoreError> {
    Ok(core::push::decrypt(&device_key_pem, &subject)?.into())
}

/// Caller name from a call push subject ("Bob möchte mit dir sprechen" → "Bob").
#[uniffi::export]
pub fn push_caller_name(subject: String) -> String {
    core::push::caller_from_subject(&subject)
}

#[uniffi::export(async_runtime = "tokio")]
impl TalkSession {
    /// Registers for pushes at the server and the proxy (every start and on token change).
    /// iOS: `push_token` is "<apns hex> <pushkit hex>".
    pub async fn register_push(
        &self,
        push_token: String,
        proxy_url: String,
        device_key_pem: String,
    ) -> Result<PushRegistration, CoreError> {
        let r = self.engine.register_push(&push_token, &proxy_url, &device_key_pem).await?;
        Ok(PushRegistration {
            device_identifier: r.device_identifier,
            user_public_key: r.user_public_key,
            proxy: r.proxy,
        })
    }

    pub async fn unregister_push(&self) -> Result<(), CoreError> {
        Ok(self.engine.unregister_push().await?)
    }

    /// Acts on a decrypted push: a call rings (`call_id`: the id already reported to
    /// CallKit), a delete re-checks a ringing call. A call that is already over still
    /// rings once and ends at once (`missed`), so a reported CallKit call gets its end.
    /// Returns `false` when nothing rings for it (already rung or declined here, or
    /// another call in progress): unless `currentCall()` is this token, end the reported
    /// CallKit call yourself.
    pub async fn handle_push(&self, message: PushMessage, call_id: Option<String>) -> bool {
        self.engine.handle_push(message.into(), call_id).await
    }
}
