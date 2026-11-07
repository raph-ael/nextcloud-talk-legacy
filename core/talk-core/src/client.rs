//! HTTP client for the Nextcloud OCS and Talk (spreed) APIs.
//!
//! One `TalkClient` per account: it owns its own connection pool and cookie jar
//! (Talk binds room sessions to the PHP session cookie), so a slow or unreachable
//! account never stalls another one.

use std::time::Duration;

use reqwest::header::{self, HeaderMap};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::models::{ChatMessage, Room, SignalingSettings};

const SPREED: &str = "/ocs/v2.php/apps/spreed/api";

/// Which platform we present ourselves as. The notifications app only routes Talk
/// pushes to devices whose user agent matches the official Talk app pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Android,
    Ios,
    Desktop,
    Cli,
}

/// Talk version we claim in the user agent. The server (spreed
/// `CanUseTalkMiddleware`) rejects official-app user agents below a minimum version
/// with HTTP 426, and gates features on it:
/// - Android/iOS ≥ 15 required, ≥ 18 when recording consent is enforced,
///   Android ≥ 25 / iOS ≥ 23 when end-to-end encrypted calls are enabled.
/// - Desktop ≥ 1.0.0, ≥ 1.1.0 with E2EE calls.
///
/// Raise these only together with implementing the gated feature.
pub const TALK_COMPAT_MOBILE: &str = "22.0.0";
pub const TALK_COMPAT_DESKTOP: &str = "1.0.0";

impl Platform {
    pub fn user_agent(self) -> String {
        let v = crate::VERSION;
        match self {
            Platform::Android => {
                format!(
                    "Mozilla/5.0 (Android) Nextcloud-Talk v{TALK_COMPAT_MOBILE} SirplusTalk/{v}"
                )
            }
            Platform::Ios => {
                format!("Mozilla/5.0 (iOS) Nextcloud-Talk v{TALK_COMPAT_MOBILE} SirplusTalk/{v}")
            }
            Platform::Desktop => {
                format!(
                    "Mozilla/5.0 (Desktop) Nextcloud-Talk v{TALK_COMPAT_DESKTOP} SirplusTalk/{v}"
                )
            }
            Platform::Cli => format!("SirplusTalk-CLI/{v}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Account {
    /// Base URL without trailing slash, e.g. `https://cloud.sirplus.org`.
    pub server: String,
    pub user: String,
    pub app_password: String,
}

#[derive(Deserialize)]
struct OcsEnvelope<T> {
    ocs: OcsBody<T>,
}

#[derive(Deserialize)]
struct OcsBody<T> {
    data: T,
}

/// Result of a chat poll.
#[derive(Debug, Clone, Default)]
pub struct ChatBatch {
    /// Messages in ascending id order.
    pub messages: Vec<ChatMessage>,
    /// Value of `X-Chat-Last-Given`: pass as `lastKnownMessageId` for the next call.
    pub last_given: Option<i64>,
    /// Value of `X-Chat-Last-Common-Read`.
    pub last_common_read: Option<i64>,
}

/// Result of a room list fetch.
#[derive(Debug, Clone)]
pub struct RoomsBatch {
    pub rooms: Vec<Room>,
    /// `X-Nextcloud-Talk-Modified-Before`: pass as `modified_since` for an incremental fetch.
    pub modified_before: Option<i64>,
    /// `X-Nextcloud-Talk-Hash`: when it changes, capabilities must be refetched.
    pub talk_hash: Option<String>,
}

#[derive(Clone)]
pub struct TalkClient {
    http: reqwest::Client,
    account: Account,
}

impl TalkClient {
    pub fn new(account: Account, platform: Platform) -> Result<Self> {
        let http = base_http_builder(platform).cookie_store(true).build()?;
        Ok(Self {
            http,
            account: Account {
                server: account.server.trim_end_matches('/').to_string(),
                ..account
            },
        })
    }

    pub fn account(&self) -> &Account {
        &self.account
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.account.server, path))
            .basic_auth(&self.account.user, Some(&self.account.app_password))
            .header("OCS-APIRequest", "true")
            .header(header::ACCEPT, "application/json")
    }

    async fn ocs<T: DeserializeOwned>(&self, rb: RequestBuilder) -> Result<(T, HeaderMap)> {
        let resp = check(rb.send().await?).await?;
        let headers = resp.headers().clone();
        let bytes = resp.bytes().await?;
        let env: OcsEnvelope<T> = serde_json::from_slice(&bytes).map_err(|e| {
            Error::Decode(format!(
                "{e} in {}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(300)])
            ))
        })?;
        Ok((env.ocs.data, headers))
    }

    /// `GET /ocs/v2.php/cloud/capabilities` – raw JSON, the caller extracts what it needs.
    pub async fn capabilities(&self) -> Result<serde_json::Value> {
        let (data, _) = self
            .ocs::<serde_json::Value>(self.request(Method::GET, "/ocs/v2.php/cloud/capabilities"))
            .await?;
        Ok(data)
    }

    /// Room list. `modified_since` enables incremental sync (deletions are not reported,
    /// so do a full fetch periodically). `background` avoids marking the user online.
    pub async fn rooms(&self, modified_since: Option<i64>, background: bool) -> Result<RoomsBatch> {
        let mut rb = self.request(Method::GET, &format!("{SPREED}/v4/room"));
        if let Some(ts) = modified_since {
            rb = rb.query(&[("modifiedSince", ts)]);
        }
        if background {
            rb = rb.query(&[("noStatusUpdate", 1)]);
        }
        let (rooms, headers) = self.ocs::<Vec<Room>>(rb).await?;
        Ok(RoomsBatch {
            rooms,
            modified_before: header_i64(&headers, "X-Nextcloud-Talk-Modified-Before"),
            talk_hash: header_str(&headers, "X-Nextcloud-Talk-Hash"),
        })
    }

    /// Long-poll for messages newer than `last_known` (returns empty on timeout).
    pub async fn poll_messages(
        &self,
        token: &str,
        last_known: i64,
        timeout_s: u32,
    ) -> Result<ChatBatch> {
        let rb = self
            .request(Method::GET, &format!("{SPREED}/v1/chat/{token}"))
            .query(&[
                ("lookIntoFuture", 1i64),
                ("lastKnownMessageId", last_known),
                ("timeout", timeout_s.min(60) as i64),
                ("limit", 100),
                ("setReadMarker", 0),
                ("markNotificationsAsRead", 0),
            ])
            // Long poll must outlive the server-side timeout.
            .timeout(Duration::from_secs(timeout_s as u64 + 15));
        self.chat_request(rb).await
    }

    /// History: up to `limit` messages older than `before` (or the newest ones when `None`).
    pub async fn history(&self, token: &str, before: Option<i64>, limit: u32) -> Result<ChatBatch> {
        let mut rb = self
            .request(Method::GET, &format!("{SPREED}/v1/chat/{token}"))
            .query(&[
                ("lookIntoFuture", 0i64),
                ("limit", limit.min(200) as i64),
                ("setReadMarker", 0),
            ]);
        if let Some(id) = before {
            rb = rb.query(&[("lastKnownMessageId", id)]);
        }
        self.chat_request(rb).await
    }

    async fn chat_request(&self, rb: RequestBuilder) -> Result<ChatBatch> {
        let resp = rb.send().await?;
        if resp.status() == StatusCode::NOT_MODIFIED {
            return Ok(ChatBatch::default());
        }
        let resp = check(resp).await?;
        let last_given = header_i64(resp.headers(), "X-Chat-Last-Given");
        let last_common_read = header_i64(resp.headers(), "X-Chat-Last-Common-Read");
        let env: OcsEnvelope<Vec<ChatMessage>> = resp.json().await?;
        let mut messages = env.ocs.data;
        messages.sort_by_key(|m| m.id);
        Ok(ChatBatch {
            messages,
            last_given,
            last_common_read,
        })
    }

    /// Send a text message. `reference_id` lets the caller match the echo of its
    /// optimistic local message; use [`new_reference_id`].
    pub async fn send_message(
        &self,
        token: &str,
        text: &str,
        reference_id: &str,
        reply_to: Option<i64>,
    ) -> Result<ChatMessage> {
        let mut form: Vec<(&str, String)> = vec![
            ("message", text.to_string()),
            ("referenceId", reference_id.to_string()),
            ("actorDisplayName", String::new()),
        ];
        if let Some(id) = reply_to {
            form.push(("replyTo", id.to_string()));
        }
        let rb = self
            .request(Method::POST, &format!("{SPREED}/v1/chat/{token}"))
            .form(&form);
        Ok(self.ocs::<ChatMessage>(rb).await?.0)
    }

    pub async fn set_read_marker(&self, token: &str, last_read: i64) -> Result<()> {
        let rb = self
            .request(Method::POST, &format!("{SPREED}/v1/chat/{token}/read"))
            .form(&[("lastReadMessage", last_read)]);
        self.ocs::<serde_json::Value>(rb).await?;
        Ok(())
    }

    /// Join a conversation (creates a Talk session). Returns the session id used for signaling.
    pub async fn join_room(&self, token: &str, force: bool) -> Result<String> {
        let rb = self
            .request(
                Method::POST,
                &format!("{SPREED}/v4/room/{token}/participants/active"),
            )
            .form(&[("force", if force { "true" } else { "false" })]);
        let (data, _) = self.ocs::<serde_json::Value>(rb).await?;
        data.get("sessionId")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| Error::Decode("join response without sessionId".into()))
    }

    pub async fn leave_room(&self, token: &str) -> Result<()> {
        let rb = self.request(
            Method::DELETE,
            &format!("{SPREED}/v4/room/{token}/participants/active"),
        );
        self.ocs::<serde_json::Value>(rb).await?;
        Ok(())
    }

    /// Signaling settings incl. ticket/JWT and fresh TURN credentials. Fetch before every connect.
    pub async fn signaling_settings(&self, token: Option<&str>) -> Result<SignalingSettings> {
        let mut rb = self.request(Method::GET, &format!("{SPREED}/v3/signaling/settings"));
        if let Some(t) = token {
            rb = rb.query(&[("token", t)]);
        }
        Ok(self.ocs::<SignalingSettings>(rb).await?.0)
    }
}

/// Random reference id for optimistic sends (sha256 hex, as the official clients do).
pub fn new_reference_id() -> String {
    let digest = Sha256::digest(uuid::Uuid::new_v4().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn base_http_builder(platform: Platform) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(platform.user_agent())
        // Short connect timeout: fail fast on broken IPv6 routes instead of hanging.
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(30))
}

async fn check(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    Err(match status {
        StatusCode::UNAUTHORIZED => Error::Unauthorized,
        StatusCode::NOT_FOUND => Error::NotFound,
        _ => Error::Http {
            status: status.as_u16(),
            message: resp
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect(),
        },
    })
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_string)
}

fn header_i64(headers: &HeaderMap, name: &str) -> Option<i64> {
    header_str(headers, name)?.parse().ok()
}
