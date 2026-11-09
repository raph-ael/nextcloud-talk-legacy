//! HTTP client for the Nextcloud OCS and Talk (spreed) APIs.
//!
//! One `TalkClient` per account: it owns its own connection pool and cookie jar
//! (Talk binds room sessions to the PHP session cookie), so a slow or unreachable
//! account never stalls another one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

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
    /// Base URL without trailing slash, e.g. `https://cloud.example.org`.
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

#[derive(Debug, Clone, Deserialize)]
pub struct CurrentUser {
    pub id: String,
    #[serde(rename = "displayname", default)]
    pub display_name: String,
}

#[derive(Clone)]
pub struct TalkClient {
    http: reqwest::Client,
    /// HTTP/1.1 only, for websocket upgrades.
    ws_http: reqwest::Client,
    account: Account,
}

impl TalkClient {
    pub fn new(account: Account, platform: Platform) -> Result<Self> {
        let http = base_http_builder(platform).cookie_store(true).build()?;
        let ws_http = ws_http_builder(platform).build()?;
        Ok(Self {
            http,
            ws_http,
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
        let msg = self.ocs::<ChatMessage>(rb).await?.0;
        fault("send")?;
        Ok(msg)
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

    /// The authenticated user (`id` may differ from the login name, e.g. e-mail logins).
    pub async fn current_user(&self) -> Result<CurrentUser> {
        let (data, _) = self
            .ocs::<CurrentUser>(self.request(Method::GET, "/ocs/v2.php/cloud/user"))
            .await?;
        Ok(data)
    }

    /// Conversation avatar (image bytes plus content type). Talk renders initials or
    /// the other user's avatar for 1:1 rooms, so this works for every room type.
    pub async fn room_avatar(&self, token: &str, dark: bool) -> Result<(Vec<u8>, String)> {
        let path = if dark {
            format!("{SPREED}/v1/room/{token}/avatar/dark")
        } else {
            format!("{SPREED}/v1/room/{token}/avatar")
        };
        let resp = check(self.request(Method::GET, &path).send().await?).await?;
        let content_type = header_str(resp.headers(), "content-type")
            .unwrap_or_else(|| "application/octet-stream".into());
        Ok((resp.bytes().await?.to_vec(), content_type))
    }

    /// Avatar of a user (call tiles). 256 px, server-rendered initials as fallback.
    pub async fn user_avatar(&self, user_id: &str, dark: bool) -> Result<(Vec<u8>, String)> {
        let user = encode_segment(user_id);
        let path = if dark {
            format!("/index.php/avatar/{user}/256/dark")
        } else {
            format!("/index.php/avatar/{user}/256")
        };
        let resp = check(self.request(Method::GET, &path).send().await?).await?;
        let content_type = header_str(resp.headers(), "content-type")
            .unwrap_or_else(|| "application/octet-stream".into());
        Ok((resp.bytes().await?.to_vec(), content_type))
    }

    /// Revokes the app password this client uses (sign-out). Afterwards every call fails
    /// with `Unauthorized`.
    pub async fn revoke_app_password(&self) -> Result<()> {
        let rb = self.request(Method::DELETE, "/ocs/v2.php/core/apppassword");
        self.ocs::<serde_json::Value>(rb).await?;
        Ok(())
    }

    /// Uploads a local file into `folder` (e.g. `/Talk`) of `user_id`'s files and returns
    /// its path relative to the user root (`/Talk/name.jpg`). Never overwrites: an existing
    /// name gets a ` (2)`, ` (3)` … suffix. Creates the folder when it is missing.
    /// One-shot helper; the outbox uses the resumable building blocks below.
    pub async fn upload(
        &self,
        user_id: &str,
        local_path: &str,
        folder: &str,
        name: &str,
        mime: &str,
    ) -> Result<String> {
        let folder = format!("/{}", folder.trim_matches('/'));
        let mut created_folder = false;
        let mut attempt = 1;
        loop {
            let remote = remote_path(&folder, name, attempt);
            match self.put_file(user_id, &remote, local_path, mime, None).await? {
                PutOutcome::Created => return Ok(remote),
                PutOutcome::Exists if attempt < 100 => attempt += 1,
                PutOutcome::ParentMissing if !created_folder => {
                    self.mkcol_all(user_id, &folder).await?;
                    created_folder = true;
                }
                other => return Err(Error::Decode(format!("upload failed: {other:?}"))),
            }
        }
    }

    /// Single WebDAV PUT of a whole local file to `remote` (path below the user root).
    /// Never overwrites (`If-None-Match: *`). `progress` receives the number of bytes
    /// handed to the network, chunk by chunk.
    pub async fn put_file(
        &self,
        user_id: &str,
        remote: &str,
        local_path: &str,
        mime: &str,
        progress: Option<Progress>,
    ) -> Result<PutOutcome> {
        let (file, len) = open_local(local_path).await?;
        let rb = self
            .dav(Method::PUT, user_id, remote)
            .header(header::CONTENT_TYPE, mime)
            // Nextcloud behind PHP-FPM stores chunked transfer-encoded uploads as empty
            // files: always send the length.
            .header(header::CONTENT_LENGTH, len)
            .header(header::IF_NONE_MATCH, "*")
            .timeout(UPLOAD_TIMEOUT);
        let (body, watch) = file_body(file, len, progress);
        let resp = stall_guard(rb.body(body).send(), watch).await?;
        Ok(match resp.status() {
            s if s.is_success() => {
                fault("put")?;
                PutOutcome::Created
            }
            StatusCode::PRECONDITION_FAILED => PutOutcome::Exists,
            StatusCode::NOT_FOUND | StatusCode::CONFLICT => PutOutcome::ParentMissing,
            _ => return Err(check(resp).await.err().unwrap_or(Error::NotFound)),
        })
    }

    /// Size of a file below the user root, `None` when it does not exist.
    pub async fn stat(&self, user_id: &str, remote: &str) -> Result<Option<u64>> {
        let resp = self
            .dav(propfind(), user_id, remote)
            .header("Depth", "0")
            .header(header::CONTENT_TYPE, "application/xml")
            .body(PROPFIND_LENGTH)
            .send()
            .await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let xml = check(resp).await?.text().await?;
        Ok(Some(
            dav_responses(&xml).first().and_then(|r| r.1).unwrap_or(0),
        ))
    }

    /// Creates `folder` and its parents (existing ones are fine).
    pub async fn mkcol_all(&self, user_id: &str, folder: &str) -> Result<()> {
        let mut path = String::new();
        for seg in folder.split('/').filter(|s| !s.is_empty()) {
            path.push('/');
            path.push_str(seg);
            let resp = self.dav(mkcol(), user_id, &path).send().await?;
            // 405: already exists.
            if !resp.status().is_success() && resp.status() != StatusCode::METHOD_NOT_ALLOWED {
                check(resp).await?;
            }
        }
        Ok(())
    }

    // --- chunked upload v2 (resumable, for large files) ------------------------------
    //
    // MKCOL uploads/{user}/{id} → PUT …/{n} per chunk → MOVE …/.file to the target.
    // Chunks live on the server until assembled (or cleaned up after about a day), so an
    // interrupted upload continues with the missing chunks only. The final MOVE is
    // atomic: the file appears completely or not at all.

    /// Starts a chunked upload session for the target `remote`. Existing sessions are fine.
    pub async fn chunk_begin(&self, user_id: &str, upload_id: &str, remote: &str) -> Result<()> {
        let resp = self
            .uploads(mkcol(), user_id, upload_id, None)
            .header("Destination", self.dav_url(user_id, remote))
            .send()
            .await?;
        if !resp.status().is_success() && resp.status() != StatusCode::METHOD_NOT_ALLOWED {
            check(resp).await?;
        }
        Ok(())
    }

    /// Chunks already on the server as (number, size); `None` when the session is gone.
    pub async fn chunk_list(&self, user_id: &str, upload_id: &str) -> Result<Option<Vec<(u32, u64)>>> {
        let resp = self
            .uploads(propfind(), user_id, upload_id, None)
            .header("Depth", "1")
            .header(header::CONTENT_TYPE, "application/xml")
            .body(PROPFIND_LENGTH)
            .send()
            .await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let xml = check(resp).await?.text().await?;
        Ok(Some(
            dav_responses(&xml)
                .into_iter()
                .filter_map(|(href, len)| {
                    let n = href.trim_end_matches('/').rsplit('/').next()?.parse().ok()?;
                    Some((n, len.unwrap_or(0)))
                })
                .collect(),
        ))
    }

    /// Uploads bytes `offset..offset+len` of `local_path` as chunk number `n` (1-based).
    #[allow(clippy::too_many_arguments)]
    pub async fn chunk_put(
        &self,
        user_id: &str,
        upload_id: &str,
        n: u32,
        local_path: &str,
        offset: u64,
        len: u64,
        remote: &str,
        total: u64,
        progress: Option<Progress>,
    ) -> Result<()> {
        let (mut file, _) = open_local(local_path).await?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| Error::LocalFile(format!("{local_path}: {e}")))?;
        let rb = self
            .uploads(Method::PUT, user_id, upload_id, Some(&n.to_string()))
            .header(header::CONTENT_LENGTH, len)
            .header("Destination", self.dav_url(user_id, remote))
            .header("OC-Total-Length", total)
            .timeout(UPLOAD_TIMEOUT);
        let (body, watch) = file_body(file, len, progress);
        check(stall_guard(rb.body(body).send(), watch).await?).await?;
        fault(&format!("chunk{n}"))?;
        Ok(())
    }

    /// Assembles the uploaded chunks into `remote`. `Exists` when the target appeared
    /// meanwhile (pick another name and call again).
    pub async fn chunk_finish(
        &self,
        user_id: &str,
        upload_id: &str,
        remote: &str,
        total: u64,
    ) -> Result<PutOutcome> {
        let resp = self
            .uploads(Method::from_bytes(b"MOVE").unwrap(), user_id, upload_id, Some(".file"))
            .header("Destination", self.dav_url(user_id, remote))
            .header("OC-Total-Length", total)
            .header("Overwrite", "F")
            // Assembling large files takes a while on the server.
            .timeout(UPLOAD_TIMEOUT)
            .send()
            .await?;
        Ok(match resp.status() {
            s if s.is_success() => {
                fault("move")?;
                PutOutcome::Created
            }
            StatusCode::PRECONDITION_FAILED => PutOutcome::Exists,
            StatusCode::CONFLICT => PutOutcome::ParentMissing,
            _ => return Err(check(resp).await.err().unwrap_or(Error::NotFound)),
        })
    }

    /// Drops a chunked upload session (best effort).
    pub async fn chunk_abort(&self, user_id: &str, upload_id: &str) -> Result<()> {
        let resp = self.uploads(Method::DELETE, user_id, upload_id, None).send().await?;
        if resp.status() != StatusCode::NOT_FOUND {
            check(resp).await?;
        }
        Ok(())
    }

    fn dav_url(&self, user_id: &str, path: &str) -> String {
        format!(
            "{}/remote.php/dav/files/{}{}",
            self.account.server,
            encode_segment(user_id),
            encode_path(path)
        )
    }

    fn dav(&self, method: Method, user_id: &str, path: &str) -> RequestBuilder {
        self.http
            .request(method, self.dav_url(user_id, path))
            .basic_auth(&self.account.user, Some(&self.account.app_password))
    }

    fn uploads(&self, method: Method, user_id: &str, upload_id: &str, item: Option<&str>) -> RequestBuilder {
        let mut url = format!(
            "{}/remote.php/dav/uploads/{}/{}",
            self.account.server,
            encode_segment(user_id),
            encode_segment(upload_id)
        );
        if let Some(i) = item {
            url.push('/');
            url.push_str(&encode_segment(i));
        }
        self.http
            .request(method, url)
            .basic_auth(&self.account.user, Some(&self.account.app_password))
    }

    /// Whether `path` (below the user root) is already shared into conversation `token`.
    /// Used before re-posting a file whose share request may have reached the server.
    pub async fn is_shared_to_room(&self, path: &str, token: &str) -> Result<bool> {
        let rb = self
            .request(Method::GET, "/ocs/v2.php/apps/files_sharing/api/v1/shares")
            .query(&[("path", path), ("reshares", "false")]);
        let shares = match self.ocs::<Vec<serde_json::Value>>(rb).await {
            Ok((s, _)) => s,
            Err(Error::NotFound) => return Ok(false),
            Err(e) => return Err(e),
        };
        Ok(shares.iter().any(|s| {
            s.get("share_type").and_then(|t| t.as_i64()) == Some(10)
                && s.get("share_with").and_then(|w| w.as_str()) == Some(token)
        }))
    }

    /// Shares an uploaded file into a conversation; Talk posts it as a file message.
    /// `reference_id` lets the caller match the echo like for text messages.
    pub async fn share_to_room(
        &self,
        token: &str,
        path: &str,
        reference_id: &str,
        caption: Option<&str>,
        reply_to: Option<i64>,
        voice: bool,
    ) -> Result<()> {
        let mut meta = serde_json::Map::new();
        // Talk keeps "voice-message" only for audio/mpeg and audio/wav files.
        let kind = if voice { "voice-message" } else { "comment" };
        meta.insert("messageType".into(), kind.into());
        if let Some(c) = caption.map(str::trim).filter(|c| !c.is_empty()) {
            meta.insert("caption".into(), c.into());
        }
        if let Some(id) = reply_to {
            meta.insert("replyTo".into(), id.into());
        }
        let rb = self
            .request(Method::POST, "/ocs/v2.php/apps/files_sharing/api/v1/shares")
            .form(&[
                ("shareType", "10".to_string()),
                ("shareWith", token.to_string()),
                ("path", path.to_string()),
                ("referenceId", reference_id.to_string()),
                ("talkMetaData", serde_json::Value::Object(meta).to_string()),
            ]);
        self.ocs::<serde_json::Value>(rb).await?;
        fault("share")
    }

    /// WebDAV GET of a file below the user root, from byte `offset` on (`Range`). The
    /// caller streams the body; 206 means resumed, 200 the whole file, 416 a bad offset.
    pub async fn download(&self, user_id: &str, remote: &str, offset: u64) -> Result<Response> {
        let mut rb = self.dav(Method::GET, user_id, remote).timeout(UPLOAD_TIMEOUT);
        if offset > 0 {
            rb = rb.header(header::RANGE, format!("bytes={offset}-"));
        }
        let resp = rb.send().await?;
        if resp.status() == StatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(resp);
        }
        check(resp).await
    }

    /// Server-rendered preview of a file, fitting into `width`×`height` (aspect kept).
    pub async fn file_preview(&self, file_id: &str, width: u32, height: u32) -> Result<Vec<u8>> {
        let rb = self.request(Method::GET, "/index.php/core/preview").query(&[
            ("fileId", file_id.to_string()),
            ("x", width.to_string()),
            ("y", height.to_string()),
            ("a", "1".to_string()),
            ("forceIcon", "0".to_string()),
        ]);
        let resp = check(rb.send().await?).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Talk attachment folder of the current user (`/Talk` unless configured otherwise).
    pub async fn attachment_folder(&self) -> Result<String> {
        let caps = self.capabilities().await?;
        Ok(caps
            .pointer("/capabilities/spreed/config/attachments/folder")
            .and_then(|v| v.as_str())
            .filter(|f| !f.is_empty())
            .unwrap_or("/Talk")
            .to_string())
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

// --- calls ---------------------------------------------------------------------------

/// `GET call/{token}/notification-state` result (capability `call-notification-state-api`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingState {
    /// Keep ringing.
    Ringing,
    /// The caller gave up: missed call.
    Missed,
    /// Stop: answered (here or on another device), call over or room gone.
    Dismiss,
}

impl TalkClient {
    /// Joins the call of a conversation (requires `join_room` with the same cookie jar).
    /// `flags`: 1 in call | 2 audio | 4 video. `silent`: nobody is notified.
    pub async fn join_call(&self, token: &str, flags: u32, silent: bool) -> Result<()> {
        let rb = self
            .request(Method::POST, &format!("{SPREED}/v4/call/{token}"))
            .form(&[("flags", flags.to_string()), ("silent", silent.to_string())]);
        self.ocs::<serde_json::Value>(rb).await?;
        Ok(())
    }

    /// Leaves the call; `all` ends it for everybody (moderators only).
    pub async fn leave_call(&self, token: &str, all: bool) -> Result<()> {
        let mut rb = self.request(Method::DELETE, &format!("{SPREED}/v4/call/{token}"));
        if all {
            rb = rb.form(&[("all", "true")]);
        }
        self.ocs::<serde_json::Value>(rb).await?;
        Ok(())
    }

    /// Who is in the call: `[{actorType, actorId, displayName, sessionId, …}]`.
    pub async fn call_peers(&self, token: &str) -> Result<Vec<serde_json::Value>> {
        let rb = self.request(Method::GET, &format!("{SPREED}/v4/call/{token}"));
        Ok(self.ocs::<Vec<serde_json::Value>>(rb).await?.0)
    }

    pub async fn ring_state(&self, token: &str) -> Result<RingState> {
        let rb = self.request(Method::GET, &format!("{SPREED}/v4/call/{token}/notification-state"));
        let resp = rb.send().await?;
        match resp.status().as_u16() {
            200 => Ok(RingState::Ringing),
            201 => Ok(RingState::Missed),
            404 => Ok(RingState::Dismiss),
            401 => Err(Error::Unauthorized),
            _ => Err(check(resp).await.err().unwrap_or(Error::Decode("ring state".into()))),
        }
    }

    /// One conversation (fresh, e.g. `hasCall`, `type`, `displayName`).
    pub async fn room(&self, token: &str) -> Result<Room> {
        let rb = self.request(Method::GET, &format!("{SPREED}/v4/room/{token}"));
        Ok(self.ocs::<Room>(rb).await?.0)
    }

    /// Participants of a conversation (`[{actorId, displayName, sessionIds, inCall, …}]`).
    pub async fn participants(&self, token: &str) -> Result<Vec<serde_json::Value>> {
        let rb = self.request(Method::GET, &format!("{SPREED}/v4/room/{token}/participants"));
        Ok(self.ocs::<Vec<serde_json::Value>>(rb).await?.0)
    }

    /// Notifications of the user (notifications app v2), newest first.
    pub async fn notifications(&self) -> Result<Vec<serde_json::Value>> {
        let rb = self.request(Method::GET, "/ocs/v2.php/apps/notifications/api/v2/notifications");
        Ok(self.ocs::<Vec<serde_json::Value>>(rb).await?.0)
    }

    /// Opens a websocket (signaling, notify_push) with the same TLS setup as the API.
    pub async fn websocket(&self, url: &str) -> Result<crate::call::ws::WebSocket> {
        crate::call::ws::connect(&self.ws_http, url).await
    }
}

/// Random reference id for optimistic sends (sha256 hex, as the official clients do).
pub fn new_reference_id() -> String {
    let digest = Sha256::digest(uuid::Uuid::new_v4().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Result of a WebDAV write that must not overwrite anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    Created,
    /// The target exists already (412).
    Exists,
    /// The parent folder is missing (404/409).
    ParentMissing,
}

/// Upload progress callback: receives the number of bytes just handed to the network.
pub type Progress = Arc<dyn Fn(u64) + Send + Sync>;

/// Upper bound for one upload request; stalls are caught much earlier by [`stall_guard`].
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(1800);
/// An upload whose body made no progress for this long is considered dead.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// After the last byte, the server must answer within this time.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(180);

const PROPFIND_LENGTH: &str = r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:getcontentlength/></d:prop></d:propfind>"#;

fn propfind() -> Method {
    Method::from_bytes(b"PROPFIND").unwrap()
}

fn mkcol() -> Method {
    Method::from_bytes(b"MKCOL").unwrap()
}

async fn open_local(path: &str) -> Result<(tokio::fs::File, u64)> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| Error::LocalFile(format!("{path}: {e}")))?;
    let len = file
        .metadata()
        .await
        .map_err(|e| Error::LocalFile(format!("{path}: {e}")))?
        .len();
    Ok((file, len))
}

/// Tracks when the request body last made progress and whether it is complete.
struct BodyWatch {
    start: Instant,
    /// Milliseconds since `start` of the last progress.
    last: AtomicU64,
    done: AtomicBool,
}

/// Streams `len` bytes of `file`, reporting progress, and returns a watch for [`stall_guard`].
fn file_body(file: tokio::fs::File, len: u64, progress: Option<Progress>) -> (reqwest::Body, Arc<BodyWatch>) {
    let watch = Arc::new(BodyWatch {
        start: Instant::now(),
        last: AtomicU64::new(0),
        done: AtomicBool::new(len == 0),
    });
    let w = watch.clone();
    let mut sent = 0u64;
    let stream = ReaderStream::with_capacity(file.take(len), 64 * 1024).inspect(move |r| {
        if let Ok(b) = r {
            sent += b.len() as u64;
            w.last.store(w.start.elapsed().as_millis() as u64, Ordering::Relaxed);
            if sent >= len {
                w.done.store(true, Ordering::Relaxed);
            }
            if let Some(p) = &progress {
                p(b.len() as u64);
            }
        }
    });
    (reqwest::Body::wrap_stream(stream), watch)
}

/// Fails an upload whose body stopped moving (dead connection: the OS would otherwise
/// retransmit for many minutes) or whose response does not come after the last byte.
async fn stall_guard(
    send: impl Future<Output = reqwest::Result<Response>>,
    watch: Arc<BodyWatch>,
) -> Result<Response> {
    tokio::pin!(send);
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            r = &mut send => return Ok(r?),
            _ = tick.tick() => {
                let idle = watch.start.elapsed().saturating_sub(Duration::from_millis(watch.last.load(Ordering::Relaxed)));
                let limit = if watch.done.load(Ordering::Relaxed) { RESPONSE_TIMEOUT } else { STALL_TIMEOUT };
                if idle > limit {
                    return Err(Error::Decode(format!("upload stalled for {}s", idle.as_secs())));
                }
            }
        }
    }
}

/// Test hook (feature `faults`, used by talk-cli): `TALK_FAULTS=put,share` makes the
/// named step fail once *after* the server processed it, as if the response was lost.
#[cfg(feature = "faults")]
fn fault(point: &str) -> Result<()> {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static FIRED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let wanted = std::env::var("TALK_FAULTS").unwrap_or_default();
    if !wanted.split(',').any(|w| w.trim() == point) {
        return Ok(());
    }
    let mut fired = FIRED.lock().unwrap();
    if fired.get_or_insert_with(HashSet::new).insert(point.to_string()) {
        tracing::warn!("injected fault after {point}");
        eprintln!("!! injizierter Fehler nach '{point}' (Antwort verloren)");
        return Err(Error::Decode(format!("injected fault after {point}")));
    }
    Ok(())
}

#[cfg(not(feature = "faults"))]
#[inline(always)]
fn fault(_point: &str) -> Result<()> {
    Ok(())
}

/// Splits a WebDAV multistatus into (href, getcontentlength) pairs.
fn dav_responses(xml: &str) -> Vec<(String, Option<u64>)> {
    xml.split("response>")
        .filter_map(|part| {
            let href = tag_text(part, "href>")?;
            let len = tag_text(part, "getcontentlength>").and_then(|t| t.trim().parse().ok());
            Some((href.to_string(), len))
        })
        .collect()
}

/// Text after the first `…name>` opening tag up to the next `<`.
fn tag_text<'a>(part: &'a str, name: &str) -> Option<&'a str> {
    let start = part.find(name)? + name.len();
    let rest = &part[start..];
    let end = rest.find('<')?;
    Some(&rest[..end])
}

/// `name.jpg` → `name (2).jpg` for `n == 2`; unchanged for `n == 1`.
fn numbered_name(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        _ => format!("{name} ({n})"),
    }
}

/// `{folder}/{name}` with the `n`-th numbered variant of `name`.
pub fn remote_path(folder: &str, name: &str, n: u32) -> String {
    let folder = folder.trim_end_matches('/');
    format!("{folder}/{}", numbered_name(name, n))
}

fn encode_path(path: &str) -> String {
    path.split('/').map(encode_segment).collect::<Vec<_>>().join("/")
}

/// Percent-encodes one URL path segment (everything except RFC 3986 unreserved characters).
fn encode_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_and_encoding() {
        assert_eq!(numbered_name("IMG_1.jpg", 1), "IMG_1.jpg");
        assert_eq!(numbered_name("IMG_1.jpg", 3), "IMG_1 (3).jpg");
        assert_eq!(numbered_name("README", 2), "README (2)");
        assert_eq!(encode_segment("Foto vom Mai ü.jpg"), "Foto%20vom%20Mai%20%C3%BC.jpg");
        assert_eq!(remote_path("/Talk", "a.jpg", 2), "/Talk/a (2).jpg");
        assert_eq!(remote_path("/", "a.jpg", 1), "/a.jpg");
    }

    #[test]
    fn parses_multistatus() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns"><d:response><d:href>/remote.php/dav/uploads/bot/abc/</d:href><d:propstat><d:prop/><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/uploads/bot/abc/1</d:href><d:propstat><d:prop><d:getcontentlength>10485760</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/uploads/bot/abc/2</d:href><d:propstat><d:prop><d:getcontentlength>42</d:getcontentlength></d:prop></d:propstat></d:response></d:multistatus>"#;
        let r = dav_responses(xml);
        assert_eq!(r.len(), 3);
        assert_eq!(r[1], ("/remote.php/dav/uploads/bot/abc/1".to_string(), Some(10485760)));
        assert_eq!(r[2].1, Some(42));
        assert_eq!(r[0].1, None);
    }
}

pub(crate) fn base_http_builder(platform: Platform) -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    #[cfg(target_os = "android")]
    let builder = builder.tls_certs_only(android_roots::system_roots());
    builder
        .user_agent(platform.user_agent())
        // Short connect timeout: fail fast on broken IPv6 routes instead of hanging.
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(30))
}

/// Client for websocket upgrades: HTTP/1.1, no total timeout (it would cut the upgraded
/// stream); the handshake is bounded by the caller.
fn ws_http_builder(platform: Platform) -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    #[cfg(target_os = "android")]
    let builder = builder.tls_certs_only(android_roots::system_roots());
    builder
        .user_agent(platform.user_agent())
        .http1_only()
        .connect_timeout(Duration::from_secs(5))
        .tcp_keepalive(Duration::from_secs(15))
        .tcp_nodelay(true)
}

async fn check(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    Err(match status {
        StatusCode::UNAUTHORIZED => Error::Unauthorized,
        StatusCode::NOT_FOUND => Error::NotFound,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => Error::RetryLater {
            status: status.as_u16(),
            after_secs: header_str(resp.headers(), "retry-after").and_then(|v| v.trim().parse().ok()),
        },
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

/// Android: trust the system CA store without the JNI-based platform verifier.
/// (User-installed CAs are not readable by apps; they would need the platform verifier.)
#[cfg(target_os = "android")]
mod android_roots {
    use std::sync::OnceLock;

    use reqwest::Certificate;

    const DIRS: &[&str] = &[
        "/apex/com.android.conscrypt/cacerts", // Android 14+ (updatable via Play)
        "/system/etc/security/cacerts",
    ];

    pub fn system_roots() -> Vec<Certificate> {
        static ROOTS: OnceLock<Vec<Certificate>> = OnceLock::new();
        ROOTS
            .get_or_init(|| {
                for dir in DIRS {
                    let Ok(entries) = std::fs::read_dir(dir) else { continue };
                    let certs: Vec<Certificate> = entries
                        .flatten()
                        .filter_map(|e| std::fs::read(e.path()).ok())
                        .filter_map(|pem| Certificate::from_pem(&pem).ok())
                        .collect();
                    if !certs.is_empty() {
                        tracing::debug!(dir, count = certs.len(), "loaded system CA certificates");
                        return certs;
                    }
                }
                tracing::error!("no system CA certificates found");
                Vec::new()
            })
            .clone()
    }
}
