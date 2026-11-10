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
        let (push_type, topic, priority, body) = match kind {
            Kind::Voip => (
                "voip",
                format!("{}.voip", self.topic),
                "10",
                json!({"subject": subject, "signature": signature, "type": "voip"}),
            ),
            Kind::Background => (
                "background",
                self.topic.clone(),
                "5",
                json!({"aps": {"content-available": 1}, "subject": subject, "signature": signature, "type": "background"}),
            ),
            Kind::Alert => (
                "alert",
                self.topic.clone(),
                "10",
                json!({"aps": {"mutable-content": 1, "alert": {"title": "Nextcloud Talk", "body": "Neue Nachricht"}},
                       "subject": subject, "signature": signature, "type": "alert"}),
            ),
        };
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
                // Ringing makes no sense later than this.
                .header("apns-expiration", "0")
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
