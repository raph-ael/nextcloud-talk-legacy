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
    pub async fn start_call(&self, token: String, silent: bool) -> Result<(), CoreError> {
        Ok(self.engine.start_call(&token, silent).await?)
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

    pub fn set_call_muted(&self, muted: bool) {
        self.engine.set_call_muted(muted);
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
