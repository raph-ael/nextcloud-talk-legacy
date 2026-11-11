//! Calls: records, the foreign `MediaEngine` and the session's call methods.
//! See docs/calls.md for the contract.

use std::sync::Arc;

use talk_core as core;

use crate::{CoreError, TalkSession};

#[derive(uniffi::Enum, Clone, Copy)]
pub enum CallState {
    Dialing,
    Ringing,
    Incoming,
    Connecting,
    Connected,
    Reconnecting,
    Ended,
}

impl From<core::CallState> for CallState {
    fn from(s: core::CallState) -> Self {
        match s {
            core::CallState::Dialing => Self::Dialing,
            core::CallState::Ringing => Self::Ringing,
            core::CallState::Incoming => Self::Incoming,
            core::CallState::Connecting => Self::Connecting,
            core::CallState::Connected => Self::Connected,
            core::CallState::Reconnecting => Self::Reconnecting,
            core::CallState::Ended => Self::Ended,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy)]
pub enum CallDirection {
    Outgoing,
    Incoming,
}

impl From<core::CallDirection> for CallDirection {
    fn from(d: core::CallDirection) -> Self {
        match d {
            core::CallDirection::Outgoing => Self::Outgoing,
            core::CallDirection::Incoming => Self::Incoming,
        }
    }
}

#[derive(uniffi::Enum, Clone)]
pub enum EndReason {
    Hangup,
    RemoteHangup,
    NoAnswer,
    Declined,
    AnsweredElsewhere,
    Missed,
    Failed { code: String, message: String },
}

impl From<core::EndReason> for EndReason {
    fn from(r: core::EndReason) -> Self {
        match r {
            core::EndReason::Hangup => Self::Hangup,
            core::EndReason::RemoteHangup => Self::RemoteHangup,
            core::EndReason::NoAnswer => Self::NoAnswer,
            core::EndReason::Declined => Self::Declined,
            core::EndReason::AnsweredElsewhere => Self::AnsweredElsewhere,
            core::EndReason::Missed => Self::Missed,
            core::EndReason::Failed { code, message } => Self::Failed { code, message },
        }
    }
}

/// Recording of the call; see docs/calls.md "Aufnahme".
#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum CallRecording {
    Off,
    StartingAudio,
    StartingVideo,
    Audio,
    Video,
    Failed,
}

impl From<core::CallRecording> for CallRecording {
    fn from(r: core::CallRecording) -> Self {
        match r {
            core::CallRecording::Off => Self::Off,
            core::CallRecording::StartingAudio => Self::StartingAudio,
            core::CallRecording::StartingVideo => Self::StartingVideo,
            core::CallRecording::Audio => Self::Audio,
            core::CallRecording::Video => Self::Video,
            core::CallRecording::Failed => Self::Failed,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum RecordingNoticeKind {
    Recording,
    Transcript,
    Summary,
    RecordingFailed,
    TranscriptFailed,
    SummaryFailed,
}

impl From<core::RecordingNoticeKind> for RecordingNoticeKind {
    fn from(k: core::RecordingNoticeKind) -> Self {
        match k {
            core::RecordingNoticeKind::Recording => Self::Recording,
            core::RecordingNoticeKind::Transcript => Self::Transcript,
            core::RecordingNoticeKind::Summary => Self::Summary,
            core::RecordingNoticeKind::RecordingFailed => Self::RecordingFailed,
            core::RecordingNoticeKind::TranscriptFailed => Self::TranscriptFailed,
            core::RecordingNoticeKind::SummaryFailed => Self::SummaryFailed,
        }
    }
}

/// A finished recording (transcript, summary, or a failure) waiting for the user.
#[derive(uniffi::Record, Clone)]
pub struct RecordingNotice {
    /// Pass to `share_recording` / `dismiss_recording`.
    pub id: i64,
    pub token: String,
    pub room_name: String,
    pub kind: RecordingNoticeKind,
    pub file_name: String,
    /// Unix seconds.
    pub timestamp: i64,
    /// "In Chat teilen" possible (otherwise only dismiss).
    pub can_share: bool,
}

impl From<core::RecordingNotice> for RecordingNotice {
    fn from(n: core::RecordingNotice) -> Self {
        Self {
            id: n.id,
            token: n.token,
            room_name: n.room_name,
            kind: n.kind.into(),
            file_name: n.file_name,
            timestamp: n.timestamp,
            can_share: n.can_share,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct CallParticipant {
    pub session_id: String,
    pub user_id: String,
    pub name: String,
    pub muted: bool,
    pub speaking: bool,
    pub connected: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct CallView {
    pub id: String,
    pub token: String,
    pub room_name: String,
    pub is_group: bool,
    pub direction: CallDirection,
    pub state: CallState,
    pub caller_name: String,
    /// Unix ms when the call became connected (timer).
    pub started_at: Option<i64>,
    pub muted: bool,
    pub participants: Vec<CallParticipant>,
    pub end_reason: Option<EndReason>,
    /// Recording state (also when somebody else started it): show the indicator.
    pub recording: CallRecording,
    /// We may start/stop a recording (moderator, recording backend, call running).
    pub can_record: bool,
    /// The server requires consent to recording before joining: show the dialog, then
    /// `consent_to_recording()` or `hang_up()`.
    pub recording_consent_pending: bool,
}

impl From<core::CallView> for CallView {
    fn from(c: core::CallView) -> Self {
        Self {
            id: c.id,
            token: c.token,
            room_name: c.room_name,
            is_group: c.is_group,
            direction: c.direction.into(),
            state: c.state.into(),
            caller_name: c.caller_name,
            started_at: c.started_at,
            muted: c.muted,
            participants: c
                .participants
                .into_iter()
                .map(|p| CallParticipant {
                    session_id: p.session_id,
                    user_id: p.user_id,
                    name: p.name,
                    muted: p.muted,
                    speaking: p.speaking,
                    connected: p.connected,
                })
                .collect(),
            end_reason: c.end_reason.map(Into::into),
            recording: c.recording.into(),
            can_record: c.can_record,
            recording_consent_pending: c.recording_consent_pending,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy)]
pub enum PeerKind {
    Publisher,
    Subscriber,
    Mesh,
}

#[derive(uniffi::Enum, Clone, Copy)]
pub enum SdpType {
    Offer,
    Answer,
}

impl From<SdpType> for core::SdpType {
    fn from(t: SdpType) -> Self {
        match t {
            SdpType::Offer => Self::Offer,
            SdpType::Answer => Self::Answer,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy)]
pub enum PeerState {
    New,
    Connecting,
    Connected,
    Disconnected,
    Failed,
    Closed,
}

impl From<PeerState> for core::PeerState {
    fn from(s: PeerState) -> Self {
        match s {
            PeerState::New => Self::New,
            PeerState::Connecting => Self::Connecting,
            PeerState::Connected => Self::Connected,
            PeerState::Disconnected => Self::Disconnected,
            PeerState::Failed => Self::Failed,
            PeerState::Closed => Self::Closed,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct CallIceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_m_line_index: i32,
}

#[derive(uniffi::Record, Clone)]
pub struct CallIceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

/// WebRTC on the platform side (see docs/calls.md). Commands must return quickly and be
/// executed in order per `peer_id`; results go back through `TalkSession.media_*`.
/// Called from a background thread.
#[uniffi::export(with_foreign)]
pub trait MediaEngine: Send + Sync {
    fn create_peer(&self, peer_id: String, kind: PeerKind, ice_servers: Vec<CallIceServer>);
    fn create_offer(&self, peer_id: String, ice_restart: bool);
    fn create_answer(&self, peer_id: String);
    fn set_remote_description(&self, peer_id: String, sdp_type: SdpType, sdp: String);
    fn add_remote_candidate(&self, peer_id: String, candidate: CallIceCandidate);
    fn close_peer(&self, peer_id: String);
    fn set_microphone_enabled(&self, enabled: bool);
    fn start_audio(&self, call_id: String);
    fn stop_audio(&self, call_id: String);
}

struct ForeignMedia(Arc<dyn MediaEngine>);

impl core::MediaEngine for ForeignMedia {
    fn create_peer(&self, peer_id: String, kind: core::PeerKind, ice_servers: Vec<core::models::IceServer>) {
        let kind = match kind {
            core::PeerKind::Publisher => PeerKind::Publisher,
            core::PeerKind::Subscriber => PeerKind::Subscriber,
            core::PeerKind::Mesh => PeerKind::Mesh,
        };
        let servers = ice_servers
            .into_iter()
            .map(|s| CallIceServer {
                urls: s.urls,
                username: s.username,
                credential: s.credential,
            })
            .collect();
        self.0.create_peer(peer_id, kind, servers);
    }
    fn create_offer(&self, peer_id: String, ice_restart: bool) {
        self.0.create_offer(peer_id, ice_restart);
    }
    fn create_answer(&self, peer_id: String) {
        self.0.create_answer(peer_id);
    }
    fn set_remote_description(&self, peer_id: String, sdp_type: core::SdpType, sdp: String) {
        let t = match sdp_type {
            core::SdpType::Offer => SdpType::Offer,
            core::SdpType::Answer => SdpType::Answer,
        };
        self.0.set_remote_description(peer_id, t, sdp);
    }
    fn add_remote_candidate(&self, peer_id: String, c: core::IceCandidate) {
        self.0.add_remote_candidate(
            peer_id,
            CallIceCandidate {
                candidate: c.candidate,
                sdp_mid: c.sdp_mid,
                sdp_m_line_index: c.sdp_m_line_index,
            },
        );
    }
    fn close_peer(&self, peer_id: String) {
        self.0.close_peer(peer_id);
    }
    fn set_microphone_enabled(&self, enabled: bool) {
        self.0.set_microphone_enabled(enabled);
    }
    fn start_audio(&self, call_id: String) {
        self.0.start_audio(call_id);
    }
    fn stop_audio(&self, call_id: String) {
        self.0.stop_audio(call_id);
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl TalkSession {
    /// Registers the platform's WebRTC implementation; once, before any call.
    pub fn set_media_engine(&self, media: Arc<dyn MediaEngine>) {
        self.engine.set_media_engine(Arc::new(ForeignMedia(media)));
    }

    /// The call in progress (also a ringing incoming one), if any.
    pub fn current_call(&self) -> Option<CallView> {
        self.engine.current_call().map(Into::into)
    }

    /// Calls the conversation or joins its running call. `silent`: nobody is rung
    /// (automated tests). Fails if another call is in progress.
    /// `call_id`: optional id for the call (iOS: the CallKit UUID), generated otherwise.
    pub async fn start_call(&self, token: String, silent: bool, call_id: Option<String>) -> Result<(), CoreError> {
        Ok(self.engine.start_call(&token, silent, call_id).await?)
    }

    pub async fn accept_call(&self, token: String) -> Result<(), CoreError> {
        Ok(self.engine.accept_call(&token).await?)
    }

    /// Stops ringing on this device (Talk has no decline signal).
    pub fn decline_call(&self, token: String) {
        self.engine.decline_call(&token);
    }

    /// Avatar of a call participant (`CallParticipant.user_id`).
    pub async fn user_avatar(&self, user_id: String, dark: bool) -> Result<Vec<u8>, CoreError> {
        Ok(self.engine.user_avatar(&user_id, dark).await?.0)
    }

    pub fn hang_up(&self) {
        self.engine.hang_up();
    }

    /// Hangs up and waits (at most `timeout_ms`) until the server knows we left.
    /// Call before the app quits; `shutdown` alone hangs up only best effort.
    pub async fn hang_up_and_wait(&self, timeout_ms: u32) {
        self.engine
            .hang_up_and_wait(std::time::Duration::from_millis(timeout_ms.into()))
            .await;
    }

    pub fn set_call_muted(&self, muted: bool) {
        self.engine.set_call_muted(muted);
    }

    // Recording (docs/calls.md "Aufnahme").

    /// Starts recording the current call (only when `CallView.can_record`). `video`:
    /// records the call view (avatars, screen shares); otherwise audio only.
    pub async fn start_recording(&self, video: bool) -> Result<(), CoreError> {
        Ok(self.engine.start_recording(video).await?)
    }

    pub async fn stop_recording(&self) -> Result<(), CoreError> {
        Ok(self.engine.stop_recording().await?)
    }

    /// Answer to `CallView.recording_consent_pending`: the user agrees to being recorded.
    pub fn consent_to_recording(&self) {
        self.engine.consent_to_recording();
    }

    /// Finished recordings waiting for the user (changes: `CoreEvent::RecordingsChanged`).
    pub fn recording_notices(&self) -> Vec<RecordingNotice> {
        self.engine.recording_notices().into_iter().map(Into::into).collect()
    }

    /// "In Chat teilen": posts the file into its conversation, removes the notice.
    pub async fn share_recording(&self, id: i64) -> Result<(), CoreError> {
        Ok(self.engine.share_recording(id).await?)
    }

    /// "Verwerfen": removes the notice (the file stays in the user's files).
    pub async fn dismiss_recording(&self, id: i64) -> Result<(), CoreError> {
        Ok(self.engine.dismiss_recording(id).await?)
    }

    // Reports of the MediaEngine.

    pub fn media_local_description(&self, peer_id: String, sdp_type: SdpType, sdp: String) {
        self.engine.media_report(core::MediaReport::LocalDescription {
            peer_id,
            sdp_type: sdp_type.into(),
            sdp,
        });
    }

    pub fn media_local_candidate(&self, peer_id: String, candidate: CallIceCandidate) {
        self.engine.media_report(core::MediaReport::LocalCandidate {
            peer_id,
            candidate: core::IceCandidate {
                candidate: candidate.candidate,
                sdp_mid: candidate.sdp_mid,
                sdp_m_line_index: candidate.sdp_m_line_index,
            },
        });
    }

    pub fn media_peer_state(&self, peer_id: String, state: PeerState) {
        self.engine.media_report(core::MediaReport::PeerState {
            peer_id,
            state: state.into(),
        });
    }

    pub fn media_peer_error(&self, peer_id: String, message: String) {
        self.engine
            .media_report(core::MediaReport::PeerError { peer_id, message });
    }

    /// 0..1, about every 300 ms per receiving peer and for the publisher (own mic).
    pub fn media_audio_level(&self, peer_id: String, level: f32) {
        self.engine
            .media_report(core::MediaReport::AudioLevel { peer_id, level });
    }
}
