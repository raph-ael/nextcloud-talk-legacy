//! Firebase Cloud Messaging HTTP v1 with a service account (OAuth 2 JWT bearer).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

use crate::Outcome;

#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: String,
    token_uri: String,
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

pub struct Fcm {
    account: ServiceAccount,
    key: EncodingKey,
    http: reqwest::Client,
    token: Mutex<Option<(String, SystemTime)>>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Fcm {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let account: ServiceAccount =
            serde_json::from_slice(&std::fs::read(path).with_context(|| format!("read {path}"))?)?;
        let key = EncodingKey::from_rsa_pem(account.private_key.as_bytes())?;
        tracing::info!(project = %account.project_id, "FCM enabled");
        Ok(Self {
            account,
            key,
            http: reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?,
            token: Mutex::new(None),
        })
    }

    async fn access_token(&self) -> anyhow::Result<String> {
        let mut slot = self.token.lock().await;
        if let Some((t, until)) = slot.as_ref() {
            if SystemTime::now() < *until {
                return Ok(t.clone());
            }
        }
        let iat = now();
        let claims = Claims {
            iss: &self.account.client_email,
            scope: "https://www.googleapis.com/auth/firebase.messaging",
            aud: &self.account.token_uri,
            iat,
            exp: iat + 3600,
        };
        let jwt = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.key)?;
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            expires_in: u64,
        }
        let resp: TokenResponse = self
            .http
            .post(&self.account.token_uri)
            .form(&[("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"), ("assertion", &jwt)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let until = SystemTime::now() + Duration::from_secs(resp.expires_in.saturating_sub(300));
        *slot = Some((resp.access_token.clone(), until));
        Ok(resp.access_token)
    }

    /// Data message (the app decrypts `subject` itself).
    pub async fn send(&self, token: &str, subject: &str, signature: &str, high: bool) -> Outcome {
        let access = match self.access_token().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("FCM auth: {e}");
                return Outcome::Failed;
            }
        };
        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.account.project_id
        );
        let body = json!({"message": {
            "token": token,
            "data": {"subject": subject, "signature": signature},
            "android": {"priority": if high { "high" } else { "normal" }, "ttl": "60s"}
        }});
        match self.http.post(&url).bearer_auth(access).json(&body).send().await {
            Ok(r) if r.status().is_success() => Outcome::Sent,
            Ok(r) => {
                let status = r.status();
                let text = r.text().await.unwrap_or_default();
                // Gone or never valid: the server should forget the device.
                if status == reqwest::StatusCode::NOT_FOUND
                    || text.contains("UNREGISTERED")
                    || text.contains("not a valid FCM registration token")
                {
                    tracing::info!("FCM: token unregistered");
                    Outcome::Unknown
                } else {
                    tracing::warn!(%status, "FCM: {}", text.chars().take(300).collect::<String>());
                    Outcome::Failed
                }
            }
            Err(e) => {
                tracing::warn!("FCM: {e}");
                Outcome::Failed
            }
        }
    }
}
