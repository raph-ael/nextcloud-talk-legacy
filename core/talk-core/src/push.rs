//! Push notifications (Nextcloud push v2, `apps/notifications` docs/push-v2.md).
//!
//! Each device has its own RSA key pair. The server encrypts every notification subject
//! with the device's public key and hands it to the push proxy the client chose; the
//! proxy forwards it to FCM/APNs. Only the device can read it.
//!
//! The platform keeps the private key in its keystore ([`generate_key`] once), registers
//! with [`crate::Engine::register_push`] whenever the push token changes, and decrypts
//! incoming pushes with [`decrypt`] — synchronously and without network or a running
//! session, so iOS can report a call to CallKit right inside the PushKit callback.

use base64::Engine as _;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::{Oaep, Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha512};

use crate::error::{Error, Result};

fn push_error(message: impl Into<String>) -> Error {
    Error::Call {
        code: "push".into(),
        message: message.into(),
    }
}

/// New 2048-bit device key as PKCS#8 PEM (store it in the platform keystore).
pub fn generate_key() -> Result<String> {
    let key = RsaPrivateKey::new(&mut rand_core::OsRng, 2048).map_err(|e| push_error(e.to_string()))?;
    Ok(key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| push_error(e.to_string()))?
        .to_string())
}

/// The public half in the PEM form the notifications app expects.
pub(crate) fn public_key_pem(private_pem: &str) -> Result<String> {
    let key = RsaPrivateKey::from_pkcs8_pem(private_pem).map_err(|e| push_error(format!("device key: {e}")))?;
    RsaPublicKey::from(&key)
        .to_public_key_pem(LineEnding::LF)
        .map_err(|e| push_error(e.to_string()))
}

/// `pushTokenHash`: SHA-512 of the push token, hex.
pub(crate) fn token_hash(push_token: &str) -> String {
    Sha512::digest(push_token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What a push says, after decryption.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PushMessage {
    /// Somebody calls in conversation `token`. `subject` is the server's text, e.g.
    /// "Bob would like to talk with you" (in the user's language).
    Call { token: String, notification_id: i64, subject: String },
    /// A Talk chat notification (message, mention).
    Chat { token: String, notification_id: i64, subject: String },
    /// Notifications were removed (read, answered elsewhere): stop showing / ringing.
    Delete { notification_ids: Vec<i64> },
    DeleteAll,
    /// Anything else (other apps, test pushes).
    Other { app: String, subject: String },
}

fn decrypt_raw(private_pem: &str, subject_b64: &str) -> Result<Vec<u8>> {
    let key = RsaPrivateKey::from_pkcs8_pem(private_pem).map_err(|e| push_error(format!("device key: {e}")))?;
    let data = base64::engine::general_purpose::STANDARD
        .decode(subject_b64.trim())
        .map_err(|e| push_error(format!("subject: {e}")))?;
    // Current servers use OAEP (SHA-1), older ones PKCS#1 v1.5.
    key.decrypt(Oaep::new::<sha1::Sha1>(), &data)
        .or_else(|_| key.decrypt(Pkcs1v15Encrypt, &data))
        .map_err(|e| push_error(format!("decrypt: {e}")))
}

#[derive(Deserialize)]
struct Plain {
    #[serde(default)]
    app: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    id: Value,
    #[serde(default)]
    nid: i64,
    #[serde(default)]
    nids: Vec<i64>,
    #[serde(default)]
    delete: bool,
    #[serde(default, rename = "delete-multiple")]
    delete_multiple: bool,
    #[serde(default, rename = "delete-all")]
    delete_all: bool,
}

/// Decrypts and classifies a push (`subject` as delivered by FCM/APNs, base64).
pub fn decrypt(private_pem: &str, subject_b64: &str) -> Result<PushMessage> {
    let raw = decrypt_raw(private_pem, subject_b64)?;
    let p: Plain = serde_json::from_slice(&raw).map_err(|e| push_error(format!("payload: {e}")))?;
    Ok(classify(p))
}

fn classify(p: Plain) -> PushMessage {
    if p.delete_all {
        return PushMessage::DeleteAll;
    }
    if p.delete_multiple {
        return PushMessage::Delete { notification_ids: p.nids };
    }
    if p.delete {
        return PushMessage::Delete {
            notification_ids: vec![p.nid],
        };
    }
    let token = match &p.id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    match (p.app.as_str(), p.kind.as_str()) {
        ("spreed", "call") => PushMessage::Call {
            token,
            notification_id: p.nid,
            subject: p.subject,
        },
        ("spreed", _) => PushMessage::Chat {
            token,
            notification_id: p.nid,
            subject: p.subject,
        },
        _ => PushMessage::Other {
            app: p.app,
            subject: p.subject,
        },
    }
}

/// Caller name from a call subject like "Bob would like to talk with you" /
/// "Bob möchte mit dir sprechen" (best effort, for the first CallKit report).
pub fn caller_from_subject(subject: &str) -> String {
    for marker in [" would like to talk", " möchte mit dir", " möchte mit Ihnen", " wants to talk"] {
        if let Some((name, _)) = subject.split_once(marker) {
            return name.trim().to_string();
        }
    }
    subject.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs8::DecodePublicKey;

    fn encrypt(public_pem: &str, plain: &str, oaep: bool) -> String {
        let key = RsaPublicKey::from_public_key_pem(public_pem).unwrap();
        let data = if oaep {
            key.encrypt(&mut rand_core::OsRng, Oaep::new::<sha1::Sha1>(), plain.as_bytes())
        } else {
            key.encrypt(&mut rand_core::OsRng, Pkcs1v15Encrypt, plain.as_bytes())
        }
        .unwrap();
        base64::engine::general_purpose::STANDARD.encode(data)
    }

    #[test]
    fn roundtrip_call_and_delete() {
        let private = generate_key().unwrap();
        let public = public_key_pem(&private).unwrap();
        let call = encrypt(
            &public,
            r#"{"nid":42,"app":"spreed","subject":"Bob möchte mit dir sprechen","type":"call","id":"tok123"}"#,
            true,
        );
        assert_eq!(
            decrypt(&private, &call).unwrap(),
            PushMessage::Call {
                token: "tok123".into(),
                notification_id: 42,
                subject: "Bob möchte mit dir sprechen".into()
            }
        );
        let del = encrypt(&public, r#"{"delete-multiple":true,"nids":[1,2]}"#, false);
        assert_eq!(
            decrypt(&private, &del).unwrap(),
            PushMessage::Delete {
                notification_ids: vec![1, 2]
            }
        );
        assert!(decrypt(&private, "bm9wZQ==").is_err());
        assert_eq!(caller_from_subject("Bob möchte mit dir sprechen"), "Bob");
        assert_eq!(token_hash("x").len(), 128);
    }
}

// --- registration and handling (needs a session) -------------------------------------

/// Result of registering this device for push.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRegistration {
    pub device_identifier: String,
    /// The user's public key on the server (verifies push signatures).
    pub user_public_key: String,
    pub signature: String,
    pub proxy: String,
    pub token_hash: String,
}

impl crate::Engine {
    /// Registers this device for pushes: at the Nextcloud server (which then encrypts
    /// notifications for `device_key_pem`) and at the push proxy `proxy_url`, which
    /// forwards them to `push_token` (FCM token; iOS: "<apns> <pushkit>").
    /// Call at every start and whenever the token changes; cheap when nothing changed.
    pub async fn register_push(&self, push_token: &str, proxy_url: &str, device_key_pem: &str) -> Result<PushRegistration> {
        let proxy = proxy_url.trim_end_matches('/').to_string();
        let hash = token_hash(push_token);
        let public = public_key_pem(device_key_pem)?;
        let (status, data) = self.client.register_push_device(&hash, &public, &proxy).await?;
        let reg = PushRegistration {
            device_identifier: data["deviceIdentifier"].as_str().unwrap_or_default().to_string(),
            user_public_key: data["publicKey"].as_str().unwrap_or_default().to_string(),
            signature: data["signature"].as_str().unwrap_or_default().to_string(),
            proxy: proxy.clone(),
            token_hash: hash,
        };
        if reg.device_identifier.is_empty() {
            return Err(push_error("server returned no device identifier"));
        }
        // Always (re)register at the proxy: it is idempotent, and a proxy that lost its
        // database recovers this way.
        self.client
            .register_at_proxy(&proxy, push_token, &reg.device_identifier, &reg.signature, &reg.user_public_key)
            .await?;
        tracing::info!(status, device = %&reg.device_identifier[..8.min(reg.device_identifier.len())], "push registered");
        if let Ok(json) = serde_json::to_string(&reg) {
            self.cache.set_meta("push_registration", json);
        }
        Ok(reg)
    }

    /// Stops pushes for this device (logout).
    pub async fn unregister_push(&self) -> Result<()> {
        let reg: Option<PushRegistration> = self
            .cache
            .meta("push_registration")
            .and_then(|j| serde_json::from_str(&j).ok());
        let server = self.client.unregister_push_device().await;
        if let Some(reg) = reg {
            let _ = self
                .client
                .unregister_at_proxy(&reg.proxy, &reg.device_identifier, &reg.signature, &reg.user_public_key)
                .await;
            self.cache.set_meta("push_registration", String::new());
        }
        server
    }

    /// Acts on a decrypted push: a call rings (with `call_id` if the platform already
    /// reported it, iOS/CallKit), a delete makes a ringing call re-check at once.
    /// Returns whether a call rings now.
    pub async fn handle_push(&self, message: PushMessage, call_id: Option<String>) -> bool {
        match message {
            PushMessage::Call { token, notification_id, subject } => {
                if let Some(v) = self.calls.current() {
                    // Already ringing (notify_push was faster) or in this call.
                    if v.token == token {
                        return v.state == crate::CallState::Incoming;
                    }
                }
                let caller = caller_from_subject(&subject);
                self.ring_token(&token, &caller, notification_id, call_id, false).await
            }
            PushMessage::Delete { .. } | PushMessage::DeleteAll => {
                self.calls.recheck_ring();
                false
            }
            _ => false,
        }
    }
}
