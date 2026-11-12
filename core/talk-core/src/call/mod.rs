//! Audio calls (1:1 and groups) over the Talk High Performance Backend.
//!
//! The core owns everything that can go wrong: REST joins, the signaling websocket,
//! the call state machine, retries and reconnects, and detecting incoming calls. The
//! platform only provides media through [`MediaEngine`] (WebRTC peer connections,
//! audio routing) and renders [`CallView`] from [`crate::Event::CallChanged`].
//!
//! The full contract (states, order of calls, threading rules) is in `docs/calls.md`.

use serde::Serialize;

use crate::models::IceServer;

/// What the UI shows. Exactly one call exists at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallState {
    /// Outgoing: setting up (signaling, joining). "Anrufen …"
    Dialing,
    /// Outgoing: we are in the call and the others were notified, nobody joined yet.
    /// 1:1 "Es klingelt …", group "Warte auf andere …".
    Ringing,
    /// Incoming: ringing on this device, waiting for accept/decline.
    Incoming,
    /// Accepted (or joining a running call): media is being set up.
    /// "Verbindung wird hergestellt …"
    Connecting,
    /// Talking; `started_at` is set. Timer m:ss.
    Connected,
    /// Connection lost (network, server); the core is recovering on its own.
    /// "Verbindung wird neu hergestellt …"
    Reconnecting,
    /// Final state, emitted once with `end_reason`; afterwards there is no call.
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallDirection {
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EndReason {
    /// We hung up.
    Hangup,
    /// The other side hung up (1:1), or a moderator ended the call for everyone.
    RemoteHangup,
    /// Outgoing: nobody answered within the ring timeout. "Keine Antwort"
    NoAnswer,
    /// Incoming: we declined. 1:1: the call ends for the caller too (and on our other
    /// devices). Outgoing 1:1: the callee declined.
    Declined,
    /// Incoming: answered on another device of the same account.
    AnsweredElsewhere,
    /// Incoming: the caller gave up before we answered. "Verpasster Anruf"
    Missed,
    /// Setup or recovery failed for good. `code` is stable for tests and logs
    /// (e.g. `permission`, `lobby`, `e2ee-required`, `no-microphone`, `signaling`).
    Failed { code: String, message: String },
}

/// Recording of the call (server field `callRecording`). Everybody in the call sees it;
/// only moderators can change it ([`CallView::can_record`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CallRecording {
    #[default]
    Off,
    /// Requested, the recording server is joining. "Aufnahme startet …"
    StartingAudio,
    StartingVideo,
    /// Recording. "Aufnahme läuft"
    Audio,
    Video,
    /// The recording server failed; can be started again. "Aufnahme fehlgeschlagen"
    Failed,
}

impl CallRecording {
    /// Server value: 0 none, 1 video, 2 audio, 3 video starting, 4 audio starting, 5 failed.
    pub fn from_server(v: i64) -> Self {
        match v {
            1 => Self::Video,
            2 => Self::Audio,
            3 => Self::StartingVideo,
            4 => Self::StartingAudio,
            5 => Self::Failed,
            _ => Self::Off,
        }
    }

    /// Recording or about to (the indicator is shown).
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Off | Self::Failed)
    }
}

/// Another participant (never ourselves).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallParticipant {
    /// Signaling session id: stable while the participant stays in the call.
    pub session_id: String,
    /// Nextcloud user id; empty for guests.
    pub user_id: String,
    pub name: String,
    pub muted: bool,
    pub speaking: bool,
    /// Their audio reaches us (the subscriber connection is up).
    pub connected: bool,
    /// Their camera is on (signaling `unmute`/`mute` "video", call flag 4). Show the video
    /// of `peer_id` on their tile while frames arrive; the avatar otherwise.
    pub video: bool,
    /// The connection carrying their audio and video (`MediaEngine` peer id), if any:
    /// remote video tracks of this peer belong on their tile.
    pub peer_id: Option<String>,
}

/// A screen shared by another participant (newest first in [`CallView::screens`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallScreen {
    /// Session of the participant sharing it (see [`CallParticipant::session_id`]).
    pub session_id: String,
    pub name: String,
    /// The receive-only connection carrying the screen: its video track is the screen.
    pub peer_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallView {
    /// Local id of this call (changes with every call, also used by `start_audio`).
    pub id: String,
    pub token: String,
    pub room_name: String,
    /// Group or public conversation (tiles instead of one big avatar).
    pub is_group: bool,
    pub direction: CallDirection,
    pub state: CallState,
    /// Who started the call (incoming); empty for outgoing.
    pub caller_name: String,
    /// Unix ms when the call became `Connected` (drives the timer).
    pub started_at: Option<i64>,
    pub muted: bool,
    /// Our camera is on (`set_camera_enabled`); off again when the platform reports
    /// `CameraState { on: false }` (no camera, permission denied, unplugged).
    pub camera_on: bool,
    /// We share our screen (`set_screen_share`); off again when the platform reports
    /// `ScreenState { on: false }` (stopped by the system, window closed).
    pub screen_sharing: bool,
    /// The conversation allows us to share our screen (permission "publish screen").
    pub can_share_screen: bool,
    /// Screens shared by others, newest first: the first one takes the stage.
    pub screens: Vec<CallScreen>,
    pub participants: Vec<CallParticipant>,
    pub end_reason: Option<EndReason>,
    /// Recording state of the call (also when somebody else started it).
    pub recording: CallRecording,
    /// We may start/stop a recording: the server has a recording backend, we are a
    /// (logged-in) moderator and the call is running. Controls are shown only then.
    pub can_record: bool,
    /// The server requires consent to being recorded before we join the call. The core
    /// waits (state `Connecting`); show the consent dialog, then
    /// `consent_to_recording()` or `hang_up()`.
    pub recording_consent_pending: bool,
}

// --- media engine (implemented by the platform) --------------------------------------

/// Kind of a peer connection. Camera/mic connections carry one audio and one video
/// transceiver from the start; the camera is switched by attaching or removing the track
/// (no renegotiation). Screen sharing uses connections of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PeerKind {
    /// Our own audio and video to the MCU: send-only, we create the offer.
    Publisher,
    /// Audio and video of one participant from the MCU: receive-only, the MCU offers.
    Subscriber,
    /// Without MCU: one send+receive connection per participant.
    Mesh,
    /// Our shared screen: send-only, one video transceiver (no audio), we create the
    /// offer. To the MCU (one) or, without MCU, one per participant. Attach the current
    /// screen track.
    ScreenPublisher,
    /// The screen of one participant: receive-only as offered (video, possibly audio).
    /// Mobile platforms handle it like `Subscriber`.
    ScreenSubscriber,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SdpType {
    Offer,
    Answer,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_m_line_index: i32,
}

/// Connection state of a peer (WebRTC `RTCPeerConnectionState`; use the ICE state
/// where the platform has no peer connection state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PeerState {
    New,
    Connecting,
    Connected,
    Disconnected,
    Failed,
    Closed,
}

pub type IceServerConfig = IceServer;

/// WebRTC on the platform side. All methods are commands and must return quickly;
/// results come back through the `on_*` methods of the session.
///
/// Rules:
/// - Commands for one `peer_id` must be executed strictly in order (serial queue per peer).
/// - A `peer_id` is never reused: a replaced connection gets a new id. Ignore commands
///   for unknown or closed ids, and never report for them.
/// - `create_offer`/`create_answer` also apply the description locally, then report it
///   with `on_local_description`.
/// - Audio may only start after `start_audio` (iOS: after CallKit activated the audio
///   session); `stop_audio` releases microphone, camera and audio routing.
/// - Every peer gets a video transceiver next to the audio one (publisher send-only, mesh
///   send+receive, subscriber as offered). Without camera no track is attached.
pub trait MediaEngine: Send + Sync + 'static {
    fn create_peer(&self, peer_id: String, kind: PeerKind, ice_servers: Vec<IceServerConfig>);
    fn create_offer(&self, peer_id: String, ice_restart: bool);
    fn create_answer(&self, peer_id: String);
    fn set_remote_description(&self, peer_id: String, sdp_type: SdpType, sdp: String);
    fn add_remote_candidate(&self, peer_id: String, candidate: IceCandidate);
    fn close_peer(&self, peer_id: String);
    /// Mute without renegotiation (track enabled flag).
    fn set_microphone_enabled(&self, enabled: bool);
    /// Opens (true) or closes the camera and attaches/removes its track on the video
    /// sender of every publisher and mesh peer, without renegotiation. Repeated after every
    /// new publisher/mesh peer: then only attach. Failures: `CameraState { on: false }`.
    fn set_camera_enabled(&self, enabled: bool);
    fn start_audio(&self, call_id: String);
    fn stop_audio(&self, call_id: String);
    /// Our screen share ended: stop the screen track (the system's capture indicator
    /// goes off). Platforms that cannot share ignore it.
    fn stop_screen(&self) {}
}

/// Reports from the platform's media engine to the core. JSON form (desktop):
/// `{"type":"localDescription","peerId":"…","sdpType":"offer","sdp":"…"}`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MediaReport {
    LocalDescription { peer_id: String, sdp_type: SdpType, sdp: String },
    LocalCandidate { peer_id: String, candidate: IceCandidate },
    PeerState { peer_id: String, state: PeerState },
    PeerError { peer_id: String, message: String },
    /// 0..1, about every 300 ms per receiving peer (and for the publisher: own mic).
    AudioLevel { peer_id: String, level: f32 },
    /// The camera could not be opened or stopped on its own (unplugged, taken by another
    /// app, permission denied): `on: false`. The platform shows the reason itself.
    CameraState { on: bool },
    /// The shared screen ended on its own (stopped in the system's UI, window closed,
    /// app reloaded): `on: false`.
    ScreenState { on: bool },
}

mod actor;
mod incoming;
mod manager;
mod proto;
mod recording;
pub use recording::{RecordingNotice, RecordingNoticeKind};
pub(crate) use recording::Recordings;
pub(crate) use manager::{CallManager, RoomInfo};
pub(crate) mod ws;
