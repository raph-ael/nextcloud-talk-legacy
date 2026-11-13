//! Node-API facade over `talk-core` for the Electron main process.
//!
//! Data crosses the boundary as plain JS objects (serde → napi), typed on the
//! TypeScript side in `desktop/src/shared/core.ts`. Keep this layer thin.

use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use serde_json::Value;
use talk_core as core;

fn to_napi(e: core::Error) -> Error {
    let code = match e {
        core::Error::Unauthorized => "UNAUTHORIZED",
        core::Error::Network(_) => "NETWORK",
        core::Error::LoginExpired => "LOGIN_EXPIRED",
        _ => "SERVER",
    };
    Error::new(Status::GenericFailure, format!("{code}: {e}"))
}

fn json<T: serde::Serialize>(v: &T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| Error::from_reason(e.to_string()))
}

fn open_result(r: core::Result<String>, op: core::GroupOp) -> Value {
    match r {
        Ok(token) => serde_json::json!({ "token": token, "error": null }),
        Err(e) => serde_json::json!({ "token": null, "error": core::group_error_text(op, &e) }),
    }
}

fn group_err(r: core::Result<()>, op: core::GroupOp) -> Option<String> {
    r.err().map(|e| core::group_error_text(op, &e))
}

fn reply_preview(v: Option<Value>) -> Result<Option<core::ReplyPreview>> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v)
            .map(Some)
            .map_err(|e| Error::from_reason(format!("replyTo: {e}"))),
    }
}

#[napi]
pub fn core_version() -> String {
    core::VERSION.to_string()
}

/// Random id to attach to an optimistic message; the echo carries the same id.
#[napi]
pub fn new_reference_id() -> String {
    core::new_reference_id()
}

// --- Login Flow v2 ---------------------------------------------------------------

#[napi(object)]
pub struct LoginResult {
    pub server: String,
    pub login_name: String,
    pub app_password: String,
}

#[napi]
pub struct LoginFlow {
    inner: core::LoginFlow,
    cancel: Arc<tokio::sync::Notify>,
}

/// Starts Login Flow v2; open `loginUrl` in the system browser, then `await wait()`.
#[napi]
pub async fn start_login(server: String) -> Result<LoginFlow> {
    let inner = core::LoginFlow::start(&server, core::Platform::Desktop)
        .await
        .map_err(to_napi)?;
    Ok(LoginFlow {
        inner,
        cancel: Arc::new(tokio::sync::Notify::new()),
    })
}

#[napi]
impl LoginFlow {
    #[napi(getter)]
    pub fn login_url(&self) -> String {
        self.inner.login_url.clone()
    }

    /// Resolves after approval in the browser; rejects with `CANCELLED` after `cancel()`.
    #[napi]
    pub async fn wait(&self) -> Result<LoginResult> {
        let c = tokio::select! {
            r = self.inner.wait() => r.map_err(to_napi)?,
            _ = self.cancel.notified() => {
                return Err(Error::new(Status::Cancelled, "CANCELLED: login cancelled"));
            }
        };
        Ok(LoginResult {
            server: c.server,
            login_name: c.login_name,
            app_password: c.app_password,
        })
    }

    #[napi]
    pub fn cancel(&self) {
        self.cancel.notify_one();
    }
}

// --- logging ---------------------------------------------------------------------

/// Core logs to stderr. `filter` like `RUST_LOG` (e.g. "talk_core=debug"); without one
/// `TALK_LOG` is used, else "talk_core=info". Safe to call more than once.
#[napi]
pub fn init_logging(filter: Option<String>) {
    let filter = filter
        .or_else(|| std::env::var("TALK_LOG").ok())
        .unwrap_or_else(|| "talk_core=info".into());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}

// --- Session ---------------------------------------------------------------------

type EventCallback = ThreadsafeFunction<Value, Unknown<'static>, Value, Status, false>;

struct JsSink(EventCallback);

impl core::EventSink for JsSink {
    fn emit(&self, event: core::Event) {
        match serde_json::to_value(&event) {
            Ok(v) => {
                self.0.call(v, ThreadsafeFunctionCallMode::NonBlocking);
            }
            Err(e) => tracing::error!("event serialization failed: {e}"),
        }
    }
}


/// MediaEngine of the renderer: every command is one JSON object
/// `{ cmd: "createPeer" | "createOffer" | …, peerId?, … }` (see docs/calls.md).
struct JsMedia(EventCallback);

impl JsMedia {
    fn send(&self, v: Value) {
        self.0.call(v, ThreadsafeFunctionCallMode::NonBlocking);
    }
}

impl core::MediaEngine for JsMedia {
    fn create_peer(&self, peer_id: String, kind: core::PeerKind, ice_servers: Vec<core::IceServer>) {
        self.send(serde_json::json!({"cmd": "createPeer", "peerId": peer_id, "kind": kind, "iceServers": ice_servers}));
    }
    fn create_offer(&self, peer_id: String, ice_restart: bool) {
        self.send(serde_json::json!({"cmd": "createOffer", "peerId": peer_id, "iceRestart": ice_restart}));
    }
    fn create_answer(&self, peer_id: String) {
        self.send(serde_json::json!({"cmd": "createAnswer", "peerId": peer_id}));
    }
    fn set_remote_description(&self, peer_id: String, sdp_type: core::SdpType, sdp: String) {
        self.send(serde_json::json!({"cmd": "setRemoteDescription", "peerId": peer_id, "sdpType": sdp_type, "sdp": sdp}));
    }
    fn add_remote_candidate(&self, peer_id: String, candidate: core::IceCandidate) {
        self.send(serde_json::json!({"cmd": "addRemoteCandidate", "peerId": peer_id, "candidate": candidate}));
    }
    fn close_peer(&self, peer_id: String) {
        self.send(serde_json::json!({"cmd": "closePeer", "peerId": peer_id}));
    }
    fn set_microphone_enabled(&self, enabled: bool) {
        self.send(serde_json::json!({"cmd": "setMicrophoneEnabled", "enabled": enabled}));
    }
    fn set_camera_enabled(&self, enabled: bool) {
        self.send(serde_json::json!({"cmd": "setCameraEnabled", "enabled": enabled}));
    }
    fn start_audio(&self, call_id: String) {
        self.send(serde_json::json!({"cmd": "startAudio", "callId": call_id}));
    }
    fn stop_audio(&self, call_id: String) {
        self.send(serde_json::json!({"cmd": "stopAudio", "callId": call_id}));
    }
    fn stop_screen(&self) {
        self.send(serde_json::json!({"cmd": "stopScreen"}));
    }
}

#[napi(object)]
pub struct Avatar {
    pub data: Buffer,
    pub content_type: String,
}

#[napi]
pub struct TalkSession {
    engine: Arc<core::Engine>,
}

#[napi]
impl TalkSession {
    /// `onEvent` receives every core event (`{ type: "roomsChanged", ... }`).
    #[napi(constructor)]
    pub fn new(
        server: String,
        user: String,
        app_password: String,
        #[napi(ts_arg_type = "(event: any) => void")] on_event: EventCallback,
        data_dir: Option<String>,
    ) -> Result<Self> {
        let account = core::Account {
            server,
            user,
            app_password,
        };
        if std::env::var("TALK_LOG").is_ok() {
            init_logging(None);
        }
        let client = core::TalkClient::new(account, core::Platform::Desktop).map_err(to_napi)?;
        let engine = core::Engine::new(client, Arc::new(JsSink(on_event)), data_dir.map(Into::into));
        Ok(Self { engine })
    }

    /// Resolves the user, loads conversations, starts background sync.
    #[napi]
    pub async fn start(&self) -> Result<()> {
        self.engine.start().await.map_err(to_napi)
    }

    #[napi]
    pub fn shutdown(&self) {
        self.engine.shutdown();
    }

    #[napi(getter)]
    pub fn user_id(&self) -> String {
        self.engine.user_id()
    }

    #[napi(ts_return_type = "any[]")]
    pub fn rooms(&self) -> Result<Value> {
        json(&self.engine.rooms())
    }

    #[napi]
    pub async fn refresh_rooms(&self) -> Result<()> {
        self.engine.refresh_rooms(true).await.map_err(to_napi)
    }

    /// Newest cached messages (empty without cache), for the first frame of a chat.
    #[napi(ts_return_type = "any")]
    pub fn cached_chat(&self, token: String) -> Result<Value> {
        json(&self.engine.cached_chat(&token))
    }

    #[napi(ts_return_type = "Promise<any>")]
    pub async fn open_chat(&self, token: String) -> Result<Value> {
        json(&self.engine.open_chat(&token).await.map_err(to_napi)?)
    }

    #[napi]
    pub fn close_chat(&self, token: String) {
        self.engine.close_chat(&token);
    }

    #[napi(ts_return_type = "Promise<any>")]
    pub async fn load_older(&self, token: String, before_id: i64) -> Result<Value> {
        json(&self.engine.load_older(&token, before_id).await.map_err(to_napi)?)
    }

    #[napi(ts_return_type = "Promise<any>")]
    pub async fn send(
        &self,
        token: String,
        text: String,
        reference_id: String,
        reply_to: Option<i64>,
    ) -> Result<Value> {
        let msg = self
            .engine
            .send(&token, &text, &reference_id, reply_to)
            .await
            .map_err(to_napi)?;
        json(&msg)
    }

    /// Sets the own reaction like Signal (one per person): `emoji` replaces it, the same
    /// emoji again or `null` removes it. Resolves to a short German error text to show,
    /// or `null` on success (docs/chat.md „Reaktionen“).
    #[napi]
    pub async fn set_reaction(&self, token: String, message_id: i64, emoji: Option<String>) -> Option<String> {
        self.engine
            .set_reaction(&token, message_id, emoji.as_deref())
            .await
            .err()
            .map(|e| core::reaction_error_text(&e))
    }

    // --- groups (docs/groups.md); changes resolve to a German error text or `null` ----

    #[napi]
    pub fn can_create_groups(&self) -> bool {
        self.engine.can_create_groups()
    }

    /// `UserView[]` for "Neue Nachricht"; with `excludeToken` without that group's members.
    #[napi(ts_return_type = "Promise<any>")]
    pub async fn search_users(&self, query: String, exclude_token: Option<String>) -> Result<Value> {
        json(&self.engine.search_users(&query, exclude_token.as_deref()).await.map_err(to_napi)?)
    }

    /// `{token, error}`: the one-to-one chat with a user (created when missing).
    #[napi(ts_return_type = "Promise<any>")]
    pub async fn open_direct(&self, user_id: String) -> Result<Value> {
        Ok(open_result(self.engine.open_direct(&user_id).await, core::GroupOp::OpenDirect))
    }

    /// `{token, error}`; `photo`: any image bytes (cropped to a square by the core).
    #[napi(ts_return_type = "Promise<any>")]
    pub async fn create_group(&self, name: String, user_ids: Vec<String>, photo: Option<Buffer>) -> Result<Value> {
        let photo = photo.map(|b| b.to_vec());
        Ok(open_result(self.engine.create_group(&name, &user_ids, photo).await, core::GroupOp::Create))
    }

    /// `MemberView[]`: ourselves first, then moderators, then alphabetically.
    #[napi(ts_return_type = "Promise<any>")]
    pub async fn members(&self, token: String) -> Result<Value> {
        json(&self.engine.members(&token).await.map_err(to_napi)?)
    }

    #[napi]
    pub async fn add_members(&self, token: String, user_ids: Vec<String>) -> Option<String> {
        group_err(self.engine.add_members(&token, &user_ids).await, core::GroupOp::AddMembers)
    }

    #[napi]
    pub async fn remove_member(&self, token: String, attendee_id: i64) -> Option<String> {
        group_err(self.engine.remove_member(&token, attendee_id).await, core::GroupOp::RemoveMember)
    }

    #[napi]
    pub async fn set_moderator(&self, token: String, attendee_id: i64, moderator: bool) -> Option<String> {
        group_err(self.engine.set_moderator(&token, attendee_id, moderator).await, core::GroupOp::SetModerator)
    }

    #[napi]
    pub async fn rename_group(&self, token: String, name: String) -> Option<String> {
        group_err(self.engine.rename_group(&token, &name).await, core::GroupOp::Rename)
    }

    #[napi]
    pub async fn set_description(&self, token: String, description: String) -> Option<String> {
        group_err(self.engine.set_description(&token, &description).await, core::GroupOp::Describe)
    }

    /// New group picture (any image bytes), or `null` to remove it.
    #[napi]
    pub async fn set_group_photo(&self, token: String, photo: Option<Buffer>) -> Option<String> {
        let photo = photo.map(|b| b.to_vec());
        group_err(self.engine.set_group_photo(&token, photo).await, core::GroupOp::Photo)
    }

    #[napi]
    pub async fn set_group_link(&self, token: String, on: bool) -> Option<String> {
        group_err(self.engine.set_group_link(&token, on).await, core::GroupOp::Link)
    }

    #[napi]
    pub fn group_link(&self, token: String) -> String {
        self.engine.group_link(&token)
    }

    /// 1 all messages, 2 mentions only, 3 off.
    #[napi]
    pub async fn set_notification_level(&self, token: String, level: u8) -> Option<String> {
        group_err(self.engine.set_notification_level(&token, level).await, core::GroupOp::Mute)
    }

    /// Ask when "Gruppe verlassen" is clicked: `true` → "Neuen Moderator wählen" first.
    #[napi]
    pub async fn leave_needs_moderator(&self, token: String) -> Result<bool> {
        self.engine.leave_needs_moderator(&token).await.map_err(to_napi)
    }

    #[napi]
    pub async fn leave_group(&self, token: String) -> Option<String> {
        group_err(self.engine.leave_group(&token).await, core::GroupOp::Leave)
    }

    #[napi]
    pub async fn delete_group(&self, token: String) -> Option<String> {
        group_err(self.engine.delete_group(&token).await, core::GroupOp::Delete)
    }

    /// Who reacted: `ReactionView[]`, own first, then newest first.
    #[napi(ts_return_type = "Promise<any>")]
    pub async fn reactions(&self, token: String, message_id: i64) -> Result<Value> {
        json(&self.engine.reactions(&token, message_id).await.map_err(to_napi)?)
    }

    #[napi]
    pub async fn mark_read(&self, token: String, message_id: i64) -> Result<()> {
        self.engine
            .mark_read(&token, message_id)
            .await
            .map_err(to_napi)
    }

    /// Revokes the app password on the server and stops all sync.
    #[napi]
    pub async fn logout(&self) -> Result<()> {
        self.engine.logout().await.map_err(to_napi)
    }

    // --- outbox: reliable sending ---------------------------------------------------

    /// Queues a text message; delivered in the background (retried, survives restarts).
    /// `replyTo` is a `ReplyPreview` (`{ id, author, text, isOwn, attachment }`).
    #[napi(ts_return_type = "any")]
    pub fn enqueue_text(
        &self,
        token: String,
        text: String,
        #[napi(ts_arg_type = "any")] reply_to: Option<Value>,
        reference_id: String,
    ) -> Result<Value> {
        let reply = reply_preview(reply_to)?;
        json(&self.engine.enqueue_text(&token, &text, reply, &reference_id))
    }

    /// Queues a file. `localPath` is moved into the outbox; don't use it afterwards.
    #[allow(clippy::too_many_arguments)]
    #[napi(ts_return_type = "any")]
    pub fn enqueue_file(
        &self,
        token: String,
        local_path: String,
        name: String,
        mime: String,
        width: i64,
        height: i64,
        caption: String,
        #[napi(ts_arg_type = "any")] reply_to: Option<Value>,
        reference_id: String,
    ) -> Result<Value> {
        let reply = reply_preview(reply_to)?;
        let p = self
            .engine
            .enqueue_file(
                &token,
                &local_path,
                &name,
                &mime,
                width,
                height,
                &caption,
                reply,
                &reference_id,
            )
            .map_err(to_napi)?;
        json(&p)
    }

    /// Pending and failed messages of a conversation, in send order.
    #[napi(ts_return_type = "any[]")]
    pub fn pending(&self, token: String) -> Result<Value> {
        json(&self.engine.pending(&token))
    }

    #[napi]
    pub fn retry_send(&self, reference_id: String) {
        self.engine.retry_send(&reference_id);
    }

    #[napi]
    pub fn discard(&self, reference_id: String) {
        self.engine.discard(&reference_id);
    }

    // --- calls (docs/calls.md) ---------------------------------------------------

    /// Registers the renderer's media engine: `onCommand` receives one JSON command
    /// per call, in order. Once, before any call.
    #[napi]
    pub fn set_media_engine(
        &self,
        #[napi(ts_arg_type = "(command: any) => void")] on_command: EventCallback,
    ) {
        self.engine.set_media_engine(Arc::new(JsMedia(on_command)));
    }

    /// The call in progress (also a ringing incoming one), or null.
    #[napi(ts_return_type = "any")]
    pub fn current_call(&self) -> Result<Value> {
        json(&self.engine.current_call())
    }

    /// Calls the conversation or joins its running call. `silent`: nobody is rung.
    #[napi]
    pub async fn start_call(&self, token: String, silent: bool, call_id: Option<String>) -> Result<()> {
        self.engine.start_call(&token, silent, call_id).await.map_err(to_napi)
    }

    /// Hangs up and waits (at most `timeoutMs`) until the server knows we left: await it
    /// before quitting.
    #[napi]
    pub async fn hang_up_and_wait(&self, timeout_ms: u32) {
        self.engine
            .hang_up_and_wait(std::time::Duration::from_millis(timeout_ms.into()))
            .await;
    }

    #[napi]
    pub async fn accept_call(&self, token: String) -> Result<()> {
        self.engine.accept_call(&token).await.map_err(to_napi)
    }

    #[napi]
    pub fn decline_call(&self, token: String) {
        self.engine.decline_call(&token);
    }

    /// Avatar of a call participant (`participant.userId`).
    #[napi]
    pub async fn user_avatar(&self, user_id: String, dark: bool) -> Result<Avatar> {
        let (data, content_type) = self.engine.user_avatar(&user_id, dark).await.map_err(to_napi)?;
        Ok(Avatar {
            data: data.into(),
            content_type,
        })
    }

    #[napi]
    pub fn hang_up(&self) {
        self.engine.hang_up();
    }

    #[napi]
    pub fn set_call_muted(&self, muted: bool) {
        self.engine.set_call_muted(muted);
    }

    /// Camera on/off in the running call (calls always start without camera).
    #[napi]
    pub fn set_camera_enabled(&self, on: bool) {
        self.engine.set_camera_enabled(on);
    }

    /// Screen sharing on/off in the running call. Get the screen stream first; the core
    /// then creates `screenPublisher` peers for its track (and sends `stopScreen` when
    /// the share ends for another reason).
    #[napi]
    pub fn set_screen_share(&self, on: bool) {
        self.engine.set_screen_share(on);
    }

    /// Starts recording the current call (only when `call.canRecord`). `video`: records
    /// the call view; otherwise audio only. Rejects with a readable message.
    #[napi]
    pub async fn start_recording(&self, video: bool) -> Result<()> {
        self.engine.start_recording(video).await.map_err(to_napi)
    }

    #[napi]
    pub async fn stop_recording(&self) -> Result<()> {
        self.engine.stop_recording().await.map_err(to_napi)
    }

    /// Answer to `call.recordingConsentPending`: the user agrees to being recorded.
    #[napi]
    pub fn consent_to_recording(&self) {
        self.engine.consent_to_recording();
    }

    /// Finished recordings waiting for the user (changes: event `recordingsChanged`).
    #[napi(ts_return_type = "any[]")]
    pub fn recording_notices(&self) -> Result<Value> {
        json(&self.engine.recording_notices())
    }

    #[napi]
    pub async fn share_recording(&self, id: i64) -> Result<()> {
        self.engine.share_recording(id).await.map_err(to_napi)
    }

    #[napi]
    pub async fn dismiss_recording(&self, id: i64) -> Result<()> {
        self.engine.dismiss_recording(id).await.map_err(to_napi)
    }

    /// Report of the media engine, e.g.
    /// `{ type: "localDescription", peerId, sdpType: "offer", sdp }`,
    /// `{ type: "localCandidate", peerId, candidate: { candidate, sdpMid, sdpMLineIndex } }`,
    /// `{ type: "peerState", peerId, state: "connected" }`, `{ type: "peerError", peerId, message }`,
    /// `{ type: "audioLevel", peerId, level }`, `{ type: "cameraState", on: false }`,
    /// `{ type: "screenState", on: false }`.
    #[napi]
    pub fn media_report(&self, #[napi(ts_arg_type = "any")] report: Value) -> Result<()> {
        let r: core::MediaReport =
            serde_json::from_value(report).map_err(|e| Error::from_reason(format!("media report: {e}")))?;
        self.engine.media_report(r);
        Ok(())
    }

    /// Call when connectivity changes: waiting items are retried at once.
    #[napi]
    pub fn network_changed(&self) {
        self.engine.network_changed();
    }

    #[napi]
    pub fn outbox_unsent(&self) -> u32 {
        self.engine.outbox_unsent()
    }

    /// Resolves once nothing is sending anymore (`{ waiting, failed }`).
    #[napi(ts_return_type = "Promise<any>")]
    pub async fn wait_outbox_idle(&self) -> Result<Value> {
        json(&self.engine.wait_outbox_idle().await)
    }

    /// Preview image (JPEG/PNG bytes) of a shared file, fitting into `width`×`height`.
    #[napi]
    pub async fn file_preview(&self, file_id: String, width: u32, height: u32) -> Result<Buffer> {
        let data = self
            .engine
            .file_preview(&file_id, width, height)
            .await
            .map_err(to_napi)?;
        Ok(data.into())
    }

    // --- voice messages ---------------------------------------------------------------

    /// Starts recording a voice message (16-bit mono PCM at `sampleRate` Hz).
    #[napi]
    pub fn start_voice(&self, token: String, sample_rate: u32) -> Result<VoiceRecording> {
        Ok(VoiceRecording {
            inner: self.engine.start_voice(&token, sample_rate).map_err(to_napi)?,
        })
    }

    /// The unsent recording of a conversation (`VoiceDraft` or null); recovers an
    /// interrupted one.
    #[napi(ts_return_type = "any")]
    pub fn voice_draft(&self, token: String) -> Result<Value> {
        json(&self.engine.voice_draft(&token))
    }

    #[napi]
    pub fn voice_draft_tokens(&self) -> Vec<String> {
        self.engine.voice_draft_tokens()
    }

    #[napi]
    pub fn discard_voice_draft(&self, token: String) {
        self.engine.discard_voice_draft(&token);
    }

    /// Sends the draft through the outbox as a voice message named `name` (.mp3).
    #[napi(ts_return_type = "any")]
    pub fn send_voice_draft(
        &self,
        token: String,
        name: String,
        #[napi(ts_arg_type = "any")] reply_to: Option<Value>,
        reference_id: String,
    ) -> Result<Value> {
        let reply = reply_preview(reply_to)?;
        let p = self
            .engine
            .send_voice_draft(&token, &name, reply, &reference_id)
            .map_err(to_napi)?;
        json(&p)
    }

    /// Local copy of a shared file (downloaded once, resumable, kept for offline use).
    #[napi]
    pub async fn fetch_file(&self, path: String, size: i64) -> Result<String> {
        self.engine.fetch_file(&path, size).await.map_err(to_napi)
    }

    /// `{ durationMs, waveform }` of a local audio file (decoded once, cached).
    #[napi(ts_return_type = "Promise<any>")]
    pub async fn audio_info(&self, local_path: String) -> Result<Value> {
        json(&self.engine.audio_info(&local_path).await.map_err(to_napi)?)
    }

    #[napi]
    pub async fn avatar(&self, token: String, dark: bool) -> Result<Avatar> {
        let (data, content_type) = self.engine.avatar(&token, dark).await.map_err(to_napi)?;
        Ok(Avatar {
            data: data.into(),
            content_type,
        })
    }
}

/// A recording in progress: written to disk as MP3 with every `push`.
#[napi]
pub struct VoiceRecording {
    inner: core::VoiceRecorder,
}

#[napi]
impl VoiceRecording {
    /// 16-bit little-endian mono PCM. Returns `{ durationMs, level, limitReached }`.
    #[napi(ts_return_type = "any")]
    pub fn push(&self, pcm: Buffer) -> Result<Value> {
        let samples: Vec<i16> = pcm.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        json(&self.inner.push(&samples).map_err(to_napi)?)
    }

    /// Ends the recording as the conversation's draft (null: shorter than a second).
    #[napi(ts_return_type = "any")]
    pub fn finish(&self) -> Result<Value> {
        json(&self.inner.finish().map_err(to_napi)?)
    }

    #[napi]
    pub fn cancel(&self) {
        self.inner.cancel();
    }
}

#[napi]
pub fn voice_max_duration_ms() -> u32 {
    core::voice::MAX_DURATION_MS as u32
}
