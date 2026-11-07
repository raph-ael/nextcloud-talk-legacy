//! Nextcloud Login Flow v2: works with 2FA/SSO because the user logs in in the browser.
//! <https://docs.nextcloud.com/server/latest/developer_manual/client_apis/LoginFlow/>

use std::time::{Duration, Instant};

use reqwest::StatusCode;
use serde::Deserialize;

use crate::client::{Platform, base_http_builder};
use crate::error::{Error, Result};
use crate::models::LoginCredentials;

/// Token lifetime on the server is 20 minutes.
const FLOW_LIFETIME: Duration = Duration::from_secs(20 * 60);

#[derive(Debug, Clone)]
pub struct LoginFlow {
    /// URL to open in the system browser.
    pub login_url: String,
    poll_endpoint: String,
    poll_token: String,
    started: Instant,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct StartResponse {
    poll: Poll,
    login: String,
}

#[derive(Deserialize)]
struct Poll {
    token: String,
    endpoint: String,
}

impl LoginFlow {
    /// Start a login flow against `server` (e.g. `https://cloud.sirplus.org`).
    pub async fn start(server: &str, platform: Platform) -> Result<Self> {
        let server = normalize_server(server);
        let http = base_http_builder(platform).build()?;
        let resp = http
            .post(format!("{server}/index.php/login/v2"))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(Error::Http {
                status: resp.status().as_u16(),
                message: "login flow start".into(),
            });
        }
        let start: StartResponse = resp.json().await?;
        Ok(Self {
            login_url: start.login,
            poll_endpoint: start.poll.endpoint,
            poll_token: start.poll.token,
            started: Instant::now(),
            http,
        })
    }

    /// Single poll: `Ok(None)` while the user has not approved yet.
    pub async fn poll_once(&self) -> Result<Option<LoginCredentials>> {
        if self.started.elapsed() > FLOW_LIFETIME {
            return Err(Error::LoginExpired);
        }
        let resp = self
            .http
            .post(&self.poll_endpoint)
            .form(&[("token", &self.poll_token)])
            .send()
            .await?;
        match resp.status() {
            StatusCode::NOT_FOUND => Ok(None),
            s if s.is_success() => Ok(Some(resp.json().await?)),
            s => Err(Error::Http {
                status: s.as_u16(),
                message: "login flow poll".into(),
            }),
        }
    }

    /// Poll every second until approved or expired.
    pub async fn wait(&self) -> Result<LoginCredentials> {
        loop {
            if let Some(creds) = self.poll_once().await? {
                return Ok(creds);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

/// Accepts `cloud.example.org`, `https://cloud.example.org/` etc.
pub fn normalize_server(input: &str) -> String {
    let s = input.trim().trim_end_matches('/');
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("https://{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_server;

    #[test]
    fn normalizes() {
        assert_eq!(
            normalize_server("cloud.sirplus.org/"),
            "https://cloud.sirplus.org"
        );
        assert_eq!(normalize_server(" https://x.y "), "https://x.y");
    }
}
