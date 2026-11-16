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

/// Which account a push is for. Every account has its own device key, and the push
/// itself does not name its account: the key that decrypts it does. `keys` are
/// `(account id, private key PEM)`; returns the account id and the message, or `None`
/// when no key fits (an account that was removed meanwhile).
/// Synchronous and without network like [`decrypt`].
pub fn decrypt_for(keys: &[(String, String)], subject_b64: &str) -> Option<(String, PushMessage)> {
    let data = base64::engine::general_purpose::STANDARD.decode(subject_b64.trim()).ok()?;
    let keys: Vec<(&str, RsaPrivateKey)> = keys
        .iter()
        .filter_map(|(id, pem)| Some((id.as_str(), RsaPrivateKey::from_pkcs8_pem(pem).ok()?)))
        .collect();
    let message = |raw: Vec<u8>| serde_json::from_slice::<Plain>(&raw).ok().map(classify);
    // OAEP first, over all keys: it rejects a wrong key for certain. PKCS#1 v1.5 (old
    // servers) can accept garbage from a wrong key once in a long while, so it comes
    // last, and in both cases only a payload that parses counts.
    for (id, key) in &keys {
        if let Some(m) = key.decrypt(Oaep::new::<sha1::Sha1>(), &data).ok().and_then(message) {
            return Some((id.to_string(), m));
        }
    }
    for (id, key) in &keys {
        if let Some(m) = key.decrypt(Pkcs1v15Encrypt, &data).ok().and_then(message) {
            return Some((id.to_string(), m));
        }
    }
    None
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

/// Sender and text of a chat push, for a notification built without network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPushParts {
    /// Who wrote; empty when the push does not say (reactions, reminders, invitations).
    pub sender: String,
    pub text: String,
}

/// Splits the subject of a chat push. The server writes `"{user}\n{message}"`, in
/// groups `"{user} in {conversation}\n{message}"` with the "in" in the user's language,
/// and shortens the message to fit the push. `room_name`: the conversation's name from
/// the room list (empty if unknown); with it the sender is cut out of the first line.
/// A subject without a line break (a reaction, a reminder) is all text.
pub fn chat_parts(subject: &str, room_name: &str) -> ChatPushParts {
    let Some((first, text)) = subject.split_once('\n') else {
        return ChatPushParts { sender: String::new(), text: subject.trim().to_string() };
    };
    let first = first.trim();
    let room_name = room_name.trim();
    let sender = first
        .strip_suffix(room_name)
        .filter(|head| !room_name.is_empty() && !head.trim().is_empty())
        // "Bob in " → "Bob": drop the connecting word, whatever the language.
        .and_then(|head| head.trim_end().rsplit_once(' ').map(|(name, _)| name.trim_end()))
        .filter(|name| !name.is_empty())
        .unwrap_or(first);
    ChatPushParts { sender: sender.to_string(), text: text.trim().to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs8::DecodePublicKey;

    #[test]
    fn chat_subjects_split_into_sender_and_text() {
        let parts = |subject: &str, room: &str| {
            let p = chat_parts(subject, room);
            (p.sender, p.text)
        };
        // One-to-one: the conversation is named like the sender.
        assert_eq!(parts("Bob\nHallo", "Bob"), ("Bob".into(), "Hallo".into()));
        // Groups, in any language, also with spaces in both names.
        assert_eq!(parts("Bob in Projekt\nHallo\nzweite Zeile", "Projekt"), ("Bob".into(), "Hallo\nzweite Zeile".into()));
        assert_eq!(parts("Anna Beispiel dans Jardin partagé\nSalut", "Jardin partagé"), ("Anna Beispiel".into(), "Salut".into()));
        // The conversation is not known (yet): the first line as it is.
        assert_eq!(parts("Bob in Projekt\nHallo", ""), ("Bob in Projekt".into(), "Hallo".into()));
        assert_eq!(parts("Bob in Projekt\nHallo", "Anderes"), ("Bob in Projekt".into(), "Hallo".into()));
        // Reactions, reminders and invitations have no line break.
        assert_eq!(parts("Bob hat mit 👍 reagiert", "Projekt"), ("".into(), "Bob hat mit 👍 reagiert".into()));
    }

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
    fn the_key_that_fits_names_the_account() {
        let keys: Vec<(String, String)> =
            ["a", "b", "c"].iter().map(|id| (id.to_string(), generate_key().unwrap())).collect();
        let call = r#"{"nid":7,"app":"spreed","subject":"Bob möchte mit dir sprechen","type":"call","id":"tok"}"#;
        for oaep in [true, false] {
            let push = encrypt(&public_key_pem(&keys[1].1).unwrap(), call, oaep);
            let (account, message) = decrypt_for(&keys, &push).unwrap();
            assert_eq!(account, "b");
            assert!(matches!(message, PushMessage::Call { ref token, notification_id: 7, .. } if token == "tok"));
            // The account is gone: nobody can read it.
            assert!(decrypt_for(&[keys[0].clone(), keys[2].clone()], &push).is_none());
        }
        assert!(decrypt_for(&keys, "bm9wZQ==").is_none());
        assert!(decrypt_for(&[], "bm9wZQ==").is_none());
        // A broken key in the list does not hide the others.
        let mut with_broken = vec![("x".to_string(), "kein Schlüssel".to_string())];
        with_broken.extend(keys.clone());
        let push = encrypt(&public_key_pem(&keys[2].1).unwrap(), call, true);
        assert_eq!(decrypt_for(&with_broken, &push).unwrap().0, "c");
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
    /// reported it, iOS/CallKit), a delete makes a ringing call re-check at once, and for
    /// a message the chat is fetched (the call returns when it is in the cache, after 8 s
    /// at the latest). Returns whether a call rings now.
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
            // A message or a reaction: fetch the chat now, so it is there when the user
            // follows the notification (docs/notifications.md, "Nach dem Antippen").
            PushMessage::Chat { token, .. } => {
                if let Some(engine) = self.weak_self().upgrade() {
                    engine.catch_up(&token).await;
                }
                false
            }
            _ => false,
        }
    }
}
