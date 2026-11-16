//! Apple Push Notification service over HTTP/2 with a token (.p8) key.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::json;
use tokio::sync::Mutex;

use crate::Outcome;
use crate::store::Store;

const PRODUCTION: &str = "https://api.push.apple.com";
const SANDBOX: &str = "https://api.sandbox.push.apple.com";

#[derive(Clone, Copy)]
pub enum Kind {
    /// PushKit: ring (the app must report a call to CallKit right away).
    Voip,
    /// Silent wake-up (e.g. "notification deleted": stop ringing).
    Background,
    /// Visible notification, decrypted by a Notification Service Extension.
    Alert,
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    iat: u64,
}

pub struct Apns {
    key: EncodingKey,
    key_id: String,
    team_id: String,
    topic: String,
    http: reqwest::Client,
    jwt: Mutex<Option<(String, SystemTime)>>,
    store: Store,
}

impl Apns {
    pub fn new(key_path: &str, key_id: &str, team_id: &str, topic: &str, store: Store) -> anyhow::Result<Self> {
        let key = EncodingKey::from_ec_pem(&std::fs::read(key_path)?)?;
        tracing::info!(%topic, "APNs enabled");
        Ok(Self {
            key,
            key_id: key_id.into(),
            team_id: team_id.into(),
            topic: topic.into(),
            http: reqwest::Client::builder()
                .http2_prior_knowledge()
                .timeout(Duration::from_secs(15))
                .build()?,
            jwt: Mutex::new(None),
            store,
        })
    }

    /// Provider token; Apple wants it refreshed between 20 and 60 minutes.
    async fn token(&self) -> anyhow::Result<String> {
        let mut slot = self.jwt.lock().await;
        if let Some((t, made)) = slot.as_ref() {
            if made.elapsed().unwrap_or_default() < Duration::from_secs(40 * 60) {
                return Ok(t.clone());
            }
        }
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        let iat = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let t = jsonwebtoken::encode(&header, &Claims { iss: &self.team_id, iat }, &self.key)?;
        *slot = Some((t.clone(), SystemTime::now()));
        Ok(t)
    }

    pub async fn send(&self, device_id: &str, token: &str, kind: Kind, subject: &str, signature: &str) -> Outcome {
        let jwt = match self.token().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("APNs key: {e}");
                return Outcome::Failed;
            }
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let Request { push_type, topic, priority, expiration, body } = request(kind, &self.topic, subject, signature, now);
        // Development builds have sandbox tokens; try the remembered environment first.
        let order = match self.store.apns_sandbox(device_id) {
            Some(true) => [SANDBOX, PRODUCTION],
            _ => [PRODUCTION, SANDBOX],
        };
        for host in order {
            let url = format!("{host}/3/device/{token}");
            let resp = self
                .http
                .post(&url)
                .bearer_auth(&jwt)
                .header("apns-topic", &topic)
                .header("apns-push-type", push_type)
                .header("apns-priority", priority)
                .header("apns-expiration", expiration.to_string())
                .json(&body)
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => {
                    self.store.set_apns_sandbox(device_id, host == SANDBOX);
                    return Outcome::Sent;
                }
                Ok(r) => {
                    let status = r.status();
                    let text = r.text().await.unwrap_or_default();
                    if text.contains("BadDeviceToken") {
                        // Wrong environment: try the other one.
                        continue;
                    }
                    if status == reqwest::StatusCode::GONE || text.contains("Unregistered") {
                        return Outcome::Unknown;
                    }
                    tracing::warn!(%status, "APNs: {text}");
                    return Outcome::Failed;
                }
                Err(e) => {
                    tracing::warn!("APNs: {e}");
                    return Outcome::Failed;
                }
            }
        }
        Outcome::Unknown
    }
}

/// How long a message notification may wait for a device that is offline.
pub const ALERT_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

struct Request {
    push_type: &'static str,
    topic: String,
    priority: &'static str,
    /// `apns-expiration`: 0 = one delivery attempt, otherwise a unix time until which
    /// APNs keeps the notification for an offline device.
    expiration: u64,
    body: serde_json::Value,
}

/// Headers and payload per kind. `now`: unix seconds.
fn request(kind: Kind, topic: &str, subject: &str, signature: &str, now: u64) -> Request {
    match kind {
        // Ringing makes no sense later.
        Kind::Voip => Request {
            push_type: "voip",
            topic: format!("{topic}.voip"),
            priority: "10",
            expiration: 0,
            body: json!({"subject": subject, "signature": signature, "type": "voip"}),
        },
        // "Read elsewhere": now or never. APNs keeps one notification per app for an
        // offline device, and a stored delete would push a waiting message out.
        Kind::Background => Request {
            push_type: "background",
            topic: topic.to_string(),
            priority: "5",
            expiration: 0,
            body: json!({"aps": {"content-available": 1}, "subject": subject, "signature": signature, "type": "background"}),
        },
        // A message: waits for the device. The app's notification service extension
        // decrypts `subject` and replaces the placeholder; without a title iOS shows
        // the app's name. `content-available` also wakes the app in the background when
        // iOS allows it, so it can fetch the chat before the notification is tapped.
        Kind::Alert => Request {
            push_type: "alert",
            topic: topic.to_string(),
            priority: "10",
            expiration: now + ALERT_LIFETIME.as_secs(),
            body: json!({"aps": {"mutable-content": 1, "content-available": 1, "sound": "default",
                                 "alert": {"body": "Neue Nachricht"}},
                         "subject": subject, "signature": signature, "type": "alert"}),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_messages_wait_for_an_offline_device() {
        let now = 1_800_000_000;
        let voip = request(Kind::Voip, "org.example.app", "s", "g", now);
        assert_eq!((voip.push_type, voip.topic.as_str(), voip.expiration), ("voip", "org.example.app.voip", 0));
        assert!(voip.body.get("aps").is_none());

        let delete = request(Kind::Background, "org.example.app", "s", "g", now);
        assert_eq!((delete.push_type, delete.priority, delete.expiration), ("background", "5", 0));
        assert_eq!(delete.body["aps"]["content-available"], 1);

        let message = request(Kind::Alert, "org.example.app", "s", "g", now);
        assert_eq!((message.push_type, message.priority), ("alert", "10"));
        assert_eq!(message.expiration, now + 24 * 60 * 60);
        assert_eq!(message.body["aps"]["mutable-content"], 1);
        assert_eq!(message.body["aps"]["content-available"], 1);
        assert_eq!(message.body["aps"]["sound"], "default");
        assert_eq!(message.body["aps"]["alert"]["body"], "Neue Nachricht");
        assert!(message.body["aps"]["alert"].get("title").is_none());
        assert_eq!((message.body["subject"].as_str(), message.body["signature"].as_str()), (Some("s"), Some("g")));
    }
}
