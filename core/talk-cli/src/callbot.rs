//! `talk-cli call`: a scriptable call peer for automated tests. Uses the real core
//! (signaling, state machine) with a WebRTC media engine in Rust (webrtc-rs) that sends a
//! sine tone and measures what it receives. Never shipped in the apps.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Result, bail};
use talk_core::{
    CallState, CallView, Engine, Event, EventSink, IceCandidate, IceServer, MediaEngine, MediaReport,
    PeerKind, PeerState, SdpType,
};
use tokio::sync::mpsc;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MIME_TYPE_OPUS, MediaEngine as RtcMediaEngine};
use webrtc::api::{API, APIBuilder};
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::media::Sample;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::offer_answer_options::RTCOfferOptions;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::policy::bundle_policy::RTCBundlePolicy;
use webrtc::peer_connection::policy::rtcp_mux_policy::RTCRtcpMuxPolicy;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::RTCRtpTransceiverInit;
use webrtc::rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTPCodecType};
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

enum PeerCmd {
    Create(PeerKind, Vec<IceServer>),
    Offer(bool),
    Answer,
    Remote(SdpType, String),
    Candidate(IceCandidate),
    Close,
}

/// Received audio per peer (for the test verdict).
#[derive(Default, Clone, Debug)]
pub struct RxStats {
    pub packets: u64,
    pub max_level: f32,
}

pub struct BotMedia {
    api: Arc<API>,
    engine: OnceLock<std::sync::Weak<Engine>>,
    peers: Mutex<HashMap<String, mpsc::UnboundedSender<PeerCmd>>>,
    mic_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub stats: Arc<Mutex<HashMap<String, RxStats>>>,
    tone_hz: f32,
}

impl BotMedia {
    pub fn new(tone_hz: f32) -> Result<Arc<Self>> {
        let mut m = RtcMediaEngine::default();
        m.register_default_codecs()?;
        let registry = register_default_interceptors(Registry::new(), &mut m)?;
        // Like a browser on this host: IPv4 UDP on real interfaces, no mDNS names (the MCU
        // cannot resolve them), no virtual bridges.
        let mut se = webrtc::api::setting_engine::SettingEngine::default();
        se.set_network_types(vec![webrtc::ice::network_type::NetworkType::Udp4]);
        se.set_interface_filter(Box::new(|name: &str| {
            !(name.starts_with("virbr") || name.starts_with("docker") || name.starts_with("br-") || name.starts_with("veth"))
        }));
        se.set_ice_multicast_dns_mode(webrtc::ice::mdns::MulticastDnsMode::Disabled);
        if std::env::var("BOT_ICE_LITE").is_ok() {
            se.set_lite(true);
        }
        let api = APIBuilder::new()
            .with_setting_engine(se)
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();
        Ok(Arc::new(Self {
            api: Arc::new(api),
            engine: OnceLock::new(),
            peers: Mutex::new(HashMap::new()),
            mic_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            stats: Arc::new(Mutex::new(HashMap::new())),
            tone_hz,
        }))
    }

    pub fn attach(&self, engine: &Arc<Engine>) {
        let _ = self.engine.set(Arc::downgrade(engine));
    }

    fn queue(&self, peer_id: &str, cmd: PeerCmd) {
        let mut peers = self.peers.lock().unwrap();
        if let PeerCmd::Create(..) = cmd {
            let (tx, rx) = mpsc::unbounded_channel();
            peers.insert(peer_id.to_string(), tx);
            let worker = PeerWorker {
                id: peer_id.to_string(),
                engine: self.engine.get().cloned().unwrap_or_default(),
                pc: None,
                mic: self.mic_enabled.clone(),
                stats: self.stats.clone(),
                tone_hz: self.tone_hz,
            };
            tokio::spawn(worker.run(self.api.clone(), rx));
        }
        match peers.get(peer_id) {
            Some(tx) => {
                let close = matches!(cmd, PeerCmd::Close);
                let _ = tx.send(cmd);
                if close {
                    peers.remove(peer_id);
                }
            }
            None => tracing::debug!(peer = peer_id, "command for unknown peer ignored"),
        }
    }
}

impl MediaEngine for BotMedia {
    fn create_peer(&self, peer_id: String, kind: PeerKind, ice_servers: Vec<IceServer>) {
        self.queue(&peer_id, PeerCmd::Create(kind, ice_servers));
    }
    fn create_offer(&self, peer_id: String, ice_restart: bool) {
        self.queue(&peer_id, PeerCmd::Offer(ice_restart));
    }
    fn create_answer(&self, peer_id: String) {
        self.queue(&peer_id, PeerCmd::Answer);
    }
    fn set_remote_description(&self, peer_id: String, sdp_type: SdpType, sdp: String) {
        self.queue(&peer_id, PeerCmd::Remote(sdp_type, sdp));
    }
    fn add_remote_candidate(&self, peer_id: String, candidate: IceCandidate) {
        self.queue(&peer_id, PeerCmd::Candidate(candidate));
    }
    fn close_peer(&self, peer_id: String) {
        self.queue(&peer_id, PeerCmd::Close);
    }
    fn set_microphone_enabled(&self, enabled: bool) {
        self.mic_enabled.store(enabled, std::sync::atomic::Ordering::Relaxed);
    }
    fn start_audio(&self, call_id: String) {
        tracing::info!(call = %call_id, "bot: audio on");
    }
    fn stop_audio(&self, call_id: String) {
        tracing::info!(call = %call_id, "bot: audio off");
    }
}

struct PeerWorker {
    id: String,
    engine: std::sync::Weak<Engine>,
    pc: Option<Arc<RTCPeerConnection>>,
    mic: Arc<std::sync::atomic::AtomicBool>,
    stats: Arc<Mutex<HashMap<String, RxStats>>>,
    tone_hz: f32,
}

fn report(engine: &std::sync::Weak<Engine>, r: MediaReport) {
    if let Some(e) = engine.upgrade() {
        e.media_report(r);
    }
}

impl PeerWorker {
    async fn run(mut self, api: Arc<API>, mut rx: mpsc::UnboundedReceiver<PeerCmd>) {
        while let Some(cmd) = rx.recv().await {
            let closing = matches!(cmd, PeerCmd::Close);
            if let Err(e) = self.handle(&api, cmd).await {
                tracing::warn!(peer = %self.id, "bot media: {e}");
                report(
                    &self.engine,
                    MediaReport::PeerError {
                        peer_id: self.id.clone(),
                        message: e.to_string(),
                    },
                );
            }
            if closing {
                break;
            }
        }
    }

    async fn handle(&mut self, api: &API, cmd: PeerCmd) -> Result<()> {
        match cmd {
            PeerCmd::Create(kind, servers) => {
                let config = RTCConfiguration {
                    ice_servers: servers
                        .into_iter()
                        .map(|s| RTCIceServer {
                            urls: s.urls,
                            username: s.username.unwrap_or_default(),
                            credential: s.credential.unwrap_or_default(),
                        })
                        .collect(),
                    bundle_policy: RTCBundlePolicy::MaxBundle,
                    ice_transport_policy: if std::env::var("BOT_RELAY_ONLY").is_ok() {
                        webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy::Relay
                    } else {
                        webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy::All
                    },
                    rtcp_mux_policy: RTCRtcpMuxPolicy::Require,
                    ..Default::default()
                };
                let pc = Arc::new(api.new_peer_connection(config).await?);
                self.hook(&pc);
                match kind {
                    PeerKind::Publisher | PeerKind::Mesh => {
                        let track = Arc::new(TrackLocalStaticSample::new(
                            RTCRtpCodecCapability {
                                mime_type: MIME_TYPE_OPUS.to_owned(),
                                clock_rate: 48000,
                                channels: 2,
                                ..Default::default()
                            },
                            "audio".to_owned(),
                            "talk-bot".to_owned(),
                        ));
                        let direction = if kind == PeerKind::Publisher {
                            RTCRtpTransceiverDirection::Sendonly
                        } else {
                            RTCRtpTransceiverDirection::Sendrecv
                        };
                        pc.add_transceiver_from_track(
                            Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>,
                            Some(RTCRtpTransceiverInit {
                                direction,
                                send_encodings: vec![],
                            }),
                        )
                        .await?;
                        tokio::spawn(send_tone(track, self.mic.clone(), self.tone_hz, Arc::downgrade(&pc)));
                    }
                    PeerKind::Subscriber => {}
                }
                self.pc = Some(pc);
            }
            PeerCmd::Offer(ice_restart) => {
                let pc = self.pc()?;
                let offer = pc
                    .create_offer(Some(RTCOfferOptions {
                        ice_restart,
                        ..Default::default()
                    }))
                    .await?;
                pc.set_local_description(offer.clone()).await?;
                report(
                    &self.engine,
                    MediaReport::LocalDescription {
                        peer_id: self.id.clone(),
                        sdp_type: SdpType::Offer,
                        sdp: offer.sdp,
                    },
                );
            }
            PeerCmd::Answer => {
                let pc = self.pc()?;
                let answer = pc.create_answer(None).await?;
                pc.set_local_description(answer.clone()).await?;
                report(
                    &self.engine,
                    MediaReport::LocalDescription {
                        peer_id: self.id.clone(),
                        sdp_type: SdpType::Answer,
                        sdp: answer.sdp,
                    },
                );
            }
            PeerCmd::Remote(t, sdp) => {
                // webrtc-rs gives up on a candidate pair after ~1.6 s, libwebrtc keeps
                // checking much longer. Janus answers before its ICE side is ready.
                if t == SdpType::Answer {
                    if let Some(ms) = std::env::var("BOT_ANSWER_DELAY_MS").ok().and_then(|v| v.parse().ok()) {
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                    }
                }
                let desc = match t {
                    SdpType::Offer => RTCSessionDescription::offer(sdp)?,
                    SdpType::Answer => RTCSessionDescription::answer(sdp)?,
                };
                self.pc()?.set_remote_description(desc).await?;
            }
            PeerCmd::Candidate(c) => {
                self.pc()?
                    .add_ice_candidate(RTCIceCandidateInit {
                        candidate: c.candidate,
                        sdp_mid: c.sdp_mid,
                        sdp_mline_index: Some(c.sdp_m_line_index as u16),
                        username_fragment: None,
                    })
                    .await?;
            }
            PeerCmd::Close => {
                if let Some(pc) = self.pc.take() {
                    let _ = pc.close().await;
                }
            }
        }
        Ok(())
    }

    fn pc(&self) -> Result<Arc<RTCPeerConnection>> {
        match &self.pc {
            Some(pc) => Ok(pc.clone()),
            None => bail!("no peer connection"),
        }
    }

    fn hook(&self, pc: &Arc<RTCPeerConnection>) {
        let (id, engine) = (self.id.clone(), self.engine.clone());
        pc.on_ice_candidate(Box::new(move |c| {
            let (id, engine) = (id.clone(), engine.clone());
            Box::pin(async move {
                let Some(c) = c else { return };
                let Ok(init) = c.to_json() else { return };
                report(
                    &engine,
                    MediaReport::LocalCandidate {
                        peer_id: id,
                        candidate: IceCandidate {
                            candidate: init.candidate,
                            sdp_mid: init.sdp_mid,
                            sdp_m_line_index: init.sdp_mline_index.unwrap_or(0) as i32,
                        },
                    },
                );
            })
        }));
        let (id, engine) = (self.id.clone(), self.engine.clone());
        pc.on_peer_connection_state_change(Box::new(move |s| {
            let state = match s {
                RTCPeerConnectionState::New | RTCPeerConnectionState::Unspecified => PeerState::New,
                RTCPeerConnectionState::Connecting => PeerState::Connecting,
                RTCPeerConnectionState::Connected => PeerState::Connected,
                RTCPeerConnectionState::Disconnected => PeerState::Disconnected,
                RTCPeerConnectionState::Failed => PeerState::Failed,
                RTCPeerConnectionState::Closed => PeerState::Closed,
            };
            report(
                &engine,
                MediaReport::PeerState {
                    peer_id: id.clone(),
                    state,
                },
            );
            Box::pin(async {})
        }));
        let (id, engine, stats) = (self.id.clone(), self.engine.clone(), self.stats.clone());
        pc.on_track(Box::new(move |track, _, _| {
            let (id, engine, stats) = (id.clone(), engine.clone(), stats.clone());
            Box::pin(async move {
                if track.kind() != RTPCodecType::Audio {
                    return;
                }
                tokio::spawn(receive_audio(track, id, engine, stats));
            })
        }));
    }
}

/// 20 ms Opus frames of a sine tone (silence while muted).
async fn send_tone(
    track: Arc<TrackLocalStaticSample>,
    mic: Arc<std::sync::atomic::AtomicBool>,
    hz: f32,
    pc: std::sync::Weak<RTCPeerConnection>,
) {
    let Ok(mut enc) = opus::Encoder::new(48000, opus::Channels::Stereo, opus::Application::Voip) else {
        return;
    };
    let mut phase = 0f32;
    let mut out = vec![0u8; 4000];
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    loop {
        tick.tick().await;
        if pc.upgrade().is_none() {
            return;
        }
        let on = mic.load(std::sync::atomic::Ordering::Relaxed);
        let mut pcm = vec![0f32; 960 * 2];
        for i in 0..960 {
            let v = if on { (phase).sin() * 0.3 } else { 0.0 };
            pcm[2 * i] = v;
            pcm[2 * i + 1] = v;
            phase = (phase + 2.0 * PI * hz / 48000.0) % (2.0 * PI);
        }
        let Ok(n) = enc.encode_float(&pcm, &mut out) else { continue };
        let sample = Sample {
            data: bytes::Bytes::copy_from_slice(&out[..n]),
            timestamp: SystemTime::now(),
            duration: Duration::from_millis(20),
            ..Default::default()
        };
        if track.write_sample(&sample).await.is_err() {
            return;
        }
    }
}

/// Decodes the received Opus stream and reports its level (like the apps' getStats).
async fn receive_audio(
    track: Arc<webrtc::track::track_remote::TrackRemote>,
    id: String,
    engine: std::sync::Weak<Engine>,
    stats: Arc<Mutex<HashMap<String, RxStats>>>,
) {
    let Ok(mut dec) = opus::Decoder::new(48000, opus::Channels::Stereo) else { return };
    let mut pcm = vec![0f32; 5760 * 2];
    let mut last_report = Instant::now();
    let mut peak = 0f32;
    while let Ok((pkt, _)) = track.read_rtp().await {
        if pkt.payload.is_empty() {
            continue;
        }
        let n = dec.decode_float(&pkt.payload, &mut pcm, false).unwrap_or(0);
        let rms = if n > 0 {
            (pcm[..n * 2].iter().map(|v| v * v).sum::<f32>() / (n * 2) as f32).sqrt()
        } else {
            0.0
        };
        peak = peak.max(rms);
        {
            let mut s = stats.lock().unwrap();
            let e = s.entry(id.clone()).or_default();
            e.packets += 1;
            e.max_level = e.max_level.max(rms);
        }
        if last_report.elapsed() >= Duration::from_millis(300) {
            report(&engine, MediaReport::AudioLevel { peer_id: id.clone(), level: (peak * 3.0).min(1.0) });
            peak = 0.0;
            last_report = Instant::now();
        }
    }
}

/// Prints call changes and remembers the latest view.
pub struct CallSink {
    pub latest: Mutex<Option<CallView>>,
    pub changed: tokio::sync::Notify,
    pub json: bool,
}

impl EventSink for CallSink {
    fn emit(&self, event: Event) {
        let Event::CallChanged { call } = event else { return };
        if self.json {
            println!("{}", serde_json::to_string(&call).unwrap_or_default());
        } else {
            match &call {
                Some(v) => {
                    let who: Vec<String> = v
                        .participants
                        .iter()
                        .map(|p| {
                            format!(
                                "{}{}{}{}",
                                if p.name.is_empty() { &p.session_id[..8.min(p.session_id.len())] } else { &p.name },
                                if p.connected { "" } else { " (verbinde)" },
                                if p.muted { " 🔇" } else { "" },
                                if p.speaking { " 🗣" } else { "" }
                            )
                        })
                        .collect();
                    eprintln!(
                        "call {:?}{} [{}]{}",
                        v.state,
                        v.end_reason.as_ref().map(|r| format!(" {r:?}")).unwrap_or_default(),
                        who.join(", "),
                        if v.muted { " stumm" } else { "" }
                    );
                }
                None => eprintln!("call: none"),
            }
        }
        *self.latest.lock().unwrap() = call;
        self.changed.notify_waiters();
    }
}

pub struct CallOpts {
    pub token: String,
    pub answer: bool,
    pub silent: bool,
    pub seconds: u64,
    pub expect_audio: bool,
    pub mute_after: Option<u64>,
    /// Wait this long for a call to answer (with `answer`).
    pub ring_wait: u64,
    /// Hang up if not connected after this many seconds.
    pub connect_timeout: u64,
    /// With `answer`: do not poll for calls, wait for the core's own detection.
    pub passive: bool,
}

fn own_sessions(peers: &[serde_json::Value], me: &str) -> Vec<String> {
    peers
        .iter()
        .filter(|p| p["actorId"].as_str() == Some(me))
        .filter_map(|p| p["sessionId"].as_str().map(str::to_string))
        .collect()
}

/// The bot's WebRTC: headless Chrome (libwebrtc, default) or webrtc-rs.
pub enum Media {
    Chrome(Arc<crate::chromebot::ChromeMedia>),
    Rust(Arc<BotMedia>),
}

impl Media {
    async fn stats(&self) -> HashMap<String, RxStats> {
        match self {
            Media::Chrome(c) => c.stats().await,
            Media::Rust(r) => r.stats.lock().unwrap().clone(),
        }
    }
}

pub async fn run(engine: Arc<Engine>, sink: Arc<CallSink>, media: Media, o: CallOpts) -> Result<()> {
    let me = engine.user_id();
    // Stale sessions of an earlier (killed) run are not ours to check afterwards.
    let before = own_sessions(&engine.client().call_peers(&o.token).await.unwrap_or_default(), &me);
    if !before.is_empty() {
        eprintln!("Hinweis: {} alte eigene Sitzung(en) im Anruf (verfallen serverseitig)", before.len());
    }
    // The manager's view is set synchronously when a call starts (the sink only after the
    // first event), so it never reads "no call" for a call that is just starting.
    let eng = engine.clone();
    let state = move |_: &CallSink| eng.current_call();
    if o.answer {
        eprintln!("warte auf eingehenden Anruf in {} …", o.token);
        let deadline = Instant::now() + Duration::from_secs(o.ring_wait);
        loop {
            if let Some(v) = state(&sink) {
                if v.state == CallState::Incoming && v.token == o.token {
                    break;
                }
            }
            if Instant::now() > deadline {
                bail!("kein eingehender Anruf");
            }
            // Passive: rely on notify_push / room list like the apps do.
            if !o.passive {
                engine.check_incoming_calls().await;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
        engine.accept_call(&o.token).await?;
    } else {
        engine.start_call(&o.token, o.silent, None).await?;
    }

    // Wait for the call to connect (or end).
    let connect_deadline = Instant::now() + Duration::from_secs(o.connect_timeout);
    let started = loop {
        match state(&sink) {
            Some(v) if v.state == CallState::Connected => break Instant::now(),
            Some(v) if v.state == CallState::Ended => bail!("Anruf beendet: {:?}", v.end_reason),
            None => bail!("Anruf weg"),
            _ => {}
        }
        if Instant::now() > connect_deadline {
            engine.hang_up();
            let end = Instant::now() + Duration::from_secs(10);
            while engine.current_call().is_some() && Instant::now() < end {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            bail!("nicht verbunden");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    eprintln!("verbunden, halte {} s", o.seconds);
    let mut muted = false;
    while started.elapsed() < Duration::from_secs(o.seconds) {
        if let Some(v) = state(&sink) {
            if v.state == CallState::Ended {
                eprintln!("Anruf von der Gegenseite beendet: {:?}", v.end_reason);
                break;
            }
        } else {
            break;
        }
        if let Some(m) = o.mute_after {
            if !muted && started.elapsed() > Duration::from_secs(m) {
                engine.set_call_muted(true);
                muted = true;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let rx: HashMap<String, RxStats> = media.stats().await;
    let mut ok = false;
    for (peer, s) in &rx {
        eprintln!("empfangen {peer}: {} Pakete, Pegel max {:.3}", s.packets, s.max_level);
        if s.packets > 50 && s.max_level > 0.05 {
            ok = true;
        }
    }
    if state(&sink).is_some_and(|v| v.state != CallState::Ended) {
        engine.hang_up();
    }
    let end_deadline = Instant::now() + Duration::from_secs(10);
    while state(&sink).is_some() && Instant::now() < end_deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let peers = engine.client().call_peers(&o.token).await.unwrap_or_default();
    let still = own_sessions(&peers, &me).iter().any(|s| !before.contains(s));
    if still {
        bail!("nach dem Auflegen noch im Anruf (Server)");
    }
    if o.expect_audio && !ok {
        bail!("kein Ton empfangen");
    }
    eprintln!("ok");
    Ok(())
}
