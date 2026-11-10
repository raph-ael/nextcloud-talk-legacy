//! The call itself: one task per call owning the signaling websocket, all peer
//! connections (through the platform's [`MediaEngine`]) and every timer. Commands from
//! the UI and reports from the media engine arrive on one channel, so there is no shared
//! mutable state besides the published [`CallView`].
//!
//! Recovery ladder (each step only if the previous one did not help):
//! 1. websocket lost → resume the signaling session (peers stay up)
//! 2. resume impossible → full rejoin: new session, room, call, all peers new
//! 3. publisher ICE failed or no answer → ICE restart (same sid) → full rejoin
//! 4. subscriber failed or no offer → `requestoffer` again (new subscription)

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::sleep_until;

use super::proto::{self, CallUser};
use super::ws::{self, Incoming, WebSocket};
use super::{
    CallDirection, CallParticipant, CallState, CallView, EndReason, MediaEngine, MediaReport, PeerKind,
    PeerState, SdpType,
};
use crate::client::{RingState, TalkClient};
use crate::engine::{Event, EventSink};
use crate::error::Error;
use crate::models::{IceServer, SignalingSettings};

/// Outgoing: give up when nobody joined. Incoming: stop ringing.
pub(crate) const RING_TIMEOUT: Duration = Duration::from_secs(60);
const RING_POLL: Duration = Duration::from_secs(5);
const PUBLISHER_ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
const ICE_RESTART_TIMEOUT: Duration = Duration::from_secs(5);
/// A connection still not up this long after the answer is treated as failed.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// `disconnected` often recovers on its own; act only when it lasts.
const DISCONNECTED_GRACE: Duration = Duration::from_secs(4);
const OFFER_RETRY: Duration = Duration::from_secs(8);
/// A failed `requestoffer` (publisher not ready on the MCU) is repeated after this.
const OFFER_RETRY_AFTER_ERROR: Duration = Duration::from_secs(1);
/// Somebody who just joined is still setting up their publisher; subscribing right away
/// makes the HPB wait 10 s and fail (race in its "publisher connected" waiter).
const NEWCOMER_SUBSCRIBE_DELAY: Duration = Duration::from_millis(1500);
const MESH_OFFER_FALLBACK: Duration = Duration::from_secs(10);
const PING_INTERVAL: Duration = Duration::from_secs(30);
const PONG_TIMEOUT: Duration = Duration::from_secs(10);
const PONG_TIMEOUT_PROBE: Duration = Duration::from_secs(4);
/// The server keeps a session resumable for 30 s.
const RESUME_WINDOW: Duration = Duration::from_secs(25);
const RECONNECT_GIVE_UP: Duration = Duration::from_secs(60);
const RECONNECT_BACKOFF_MAX_S: u64 = 16;
/// Show `connected` this long after somebody joined even if no media is reported yet.
const CONNECTED_GRACE: Duration = Duration::from_secs(4);
/// Media trouble shorter than this is not shown as `reconnecting`.
const MEDIA_TROUBLE_SHOWN_AFTER: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const LEAVE_TIMEOUT: Duration = Duration::from_secs(5);
const SPEAKING_LEVEL: f32 = 0.04;
const SPEAKING_HOLD: Duration = Duration::from_millis(700);
const NAMES_REFRESH: Duration = Duration::from_secs(10);
/// 1:1: the other side's session vanished without hanging up (network, app reconnecting
/// with a new session): wait this long for them to come back before ending the call.
const VANISH_GRACE: Duration = Duration::from_secs(12);
const TICK: Duration = Duration::from_millis(250);

pub(crate) enum Cmd {
    Accept,
    Decline,
    HangUp,
    Mute(bool),
    Media(MediaReport),
    NetworkChanged,
    /// Ringing: ask the server now whether it still rings (a delete push arrived).
    RecheckRing,
    /// Display names fetched in the background (actor id → name).
    Names(HashMap<String, String>),
}

/// Published state, shared with the [`super::CallManager`].
#[derive(Default)]
pub(crate) struct Shared {
    pub view: Mutex<Option<CallView>>,
    pub tx: Mutex<Option<mpsc::UnboundedSender<Cmd>>>,
}

pub(crate) struct Ctx {
    pub client: TalkClient,
    pub sink: Arc<dyn EventSink>,
    pub media: Arc<std::sync::RwLock<Option<Arc<dyn MediaEngine>>>>,
    pub shared: Arc<Shared>,
    pub me: String,
}

enum Stop {
    End(EndReason),
    Err(Error),
}

impl From<Error> for Stop {
    fn from(e: Error) -> Self {
        Stop::Err(e)
    }
}

type Step<T> = std::result::Result<T, Stop>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owner {
    Publisher,
}

struct Publisher {
    peer_id: String,
    sid: String,
    state: PeerState,
    created: Instant,
    offer_sent_at: Option<Instant>,
    answered: bool,
    bad_since: Option<Instant>,
    restart_at: Option<Instant>,
    /// Local candidates found before the MCU answered: the HPB creates the publisher
    /// asynchronously and drops candidates for a publisher that does not exist yet.
    early_candidates: Vec<super::IceCandidate>,
}

struct Peer {
    peer_id: String,
    sid: String,
    state: PeerState,
    created: Instant,
    bad_since: Option<Instant>,
    /// Mesh: we created the offer.
    offerer: bool,
}

struct Other {
    user: CallUser,
    name: String,
    muted: bool,
    speaking_until: Option<Instant>,
    connected: bool,
    peer: Option<Peer>,
    /// MCU: last `requestoffer`; mesh: when we started waiting for their offer.
    requested_at: Option<Instant>,
    /// MCU: do not subscribe before this (newcomer still publishing).
    not_before: Option<Instant>,
}

impl Other {
    fn has_media(&self) -> bool {
        self.user.in_call & 6 != 0
    }
}

struct Reconnect {
    since: Instant,
    attempt: u32,
    next_at: Instant,
    /// State to return to once recovered.
    resume_state: CallState,
}

pub(crate) struct Actor {
    client: TalkClient,
    sink: Arc<dyn EventSink>,
    media: Arc<dyn MediaEngine>,
    media_slot: Arc<std::sync::RwLock<Option<Arc<dyn MediaEngine>>>>,
    shared: Arc<Shared>,
    rx: mpsc::UnboundedReceiver<Cmd>,
    tx: mpsc::UnboundedSender<Cmd>,
    me: String,
    nick: String,
    silent: bool,
    view: CallView,
    last_emitted: Option<CallView>,

    // signaling
    ws: Option<WebSocket>,
    settings: Option<SignalingSettings>,
    session_id: String,
    resume_id: String,
    nc_session: String,
    mcu: bool,
    msg_seq: u64,
    queued: Vec<Value>,
    last_rx: Instant,
    ping_sent: Option<(Instant, Duration)>,
    next_ping: Instant,
    reconnect: Option<Reconnect>,

    // call
    audio_started: bool,
    in_call_since: Option<Instant>,
    ever_joined: bool,
    others: HashMap<String, Other>,
    names: HashMap<String, String>,
    session_names: HashMap<String, String>,
    names_fetched: Option<Instant>,
    /// Actor ids whose session disappeared without hanging up, and since when.
    vanished: HashMap<String, Instant>,
    publisher: Option<Publisher>,
    publisher_failures: u32,
    /// `requestoffer` message id → session (to retry on errors).
    offer_requests: HashMap<String, String>,
    peer_seq: u64,
    owners: HashMap<String, Owner>,
    peer_session: HashMap<String, String>,
    someone_since: Option<Instant>,
    media_trouble_since: Option<Instant>,
    muted: bool,
    ended: Option<EndReason>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn random_id() -> String {
    // The official clients use a millisecond timestamp; anything unique per stream works.
    format!("{}", uuid::Uuid::new_v4().as_u128() % 10_000_000_000_000)
}

fn backoff(attempt: u32) -> Duration {
    let base = 1u64 << attempt.min(4);
    let base = base.min(RECONNECT_BACKOFF_MAX_S) as f64;
    let jitter = (uuid::Uuid::new_v4().as_u128() % 1000) as f64 / 1000.0;
    Duration::from_secs_f64(base * (0.5 + jitter))
}

fn failed(code: &str, message: &str) -> EndReason {
    EndReason::Failed {
        code: code.into(),
        message: message.into(),
    }
}

/// Maps a setup error to the reason shown to the user.
fn end_for(e: &Error) -> EndReason {
    match e {
        Error::Call { code, message } => failed(code, message),
        Error::Unauthorized => failed("auth", "Anmeldung abgelaufen"),
        Error::Http { status: 400, message } if message.contains("consent") => {
            failed("consent", "Dieser Anruf wird aufgezeichnet – Zustimmung nötig")
        }
        Error::Http { status: 403, .. } => failed("permission", "Keine Berechtigung für Anrufe in diesem Chat"),
        Error::Http { status: 412, .. } => failed("lobby", "Der Chat ist in der Lobby"),
        Error::NotFound => failed("not-found", "Chat nicht gefunden"),
        Error::Network(_) => failed("network", "Keine Verbindung zum Server"),
        other => failed("server", &other.to_string()),
    }
}

impl Actor {
    pub(crate) fn new(
        ctx: Ctx,
        view: CallView,
        silent: bool,
        rx: mpsc::UnboundedReceiver<Cmd>,
        tx: mpsc::UnboundedSender<Cmd>,
    ) -> Self {
        let now = Instant::now();
        let media = ctx.media.read().unwrap().clone().unwrap_or_else(|| Arc::new(NoMedia));
        Self {
            client: ctx.client,
            sink: ctx.sink,
            media,
            media_slot: ctx.media,
            shared: ctx.shared,
            rx,
            tx,
            nick: ctx.me.clone(),
            me: ctx.me,
            silent,
            view,
            last_emitted: None,
            ws: None,
            settings: None,
            session_id: String::new(),
            resume_id: String::new(),
            nc_session: String::new(),
            mcu: false,
            msg_seq: 0,
            queued: Vec::new(),
            last_rx: now,
            ping_sent: None,
            next_ping: now + PING_INTERVAL,
            reconnect: None,
            audio_started: false,
            in_call_since: None,
            ever_joined: false,
            others: HashMap::new(),
            names: HashMap::new(),
            session_names: HashMap::new(),
            names_fetched: None,
            vanished: HashMap::new(),
            publisher: None,
            publisher_failures: 0,
            offer_requests: HashMap::new(),
            peer_seq: 0,
            owners: HashMap::new(),
            peer_session: HashMap::new(),
            someone_since: None,
            media_trouble_since: None,
            muted: false,
            ended: None,
        }
    }

    fn token(&self) -> String {
        self.view.token.clone()
    }

    pub(crate) async fn run(mut self) {
        tracing::info!(call = %self.view.id, token = %self.view.token, direction = ?self.view.direction, "call started");
        self.emit();
        let reason = match self.run_inner().await {
            Ok(r) | Err(r) => r,
        };
        self.teardown(reason).await;
    }

    async fn run_inner(&mut self) -> Result<EndReason, EndReason> {
        if self.view.state == CallState::Incoming {
            self.ring().await?;
        }
        // Ringing may have started before the platform registered its media engine.
        match self.media_slot.read().unwrap().clone() {
            Some(m) => self.media = m,
            None => return Err(failed("no-media-engine", "Anrufe sind in dieser App noch nicht eingerichtet")),
        }
        self.media.start_audio(self.view.id.clone());
        self.audio_started = true;
        match self.connect_full(true).await {
            Ok(()) => {}
            Err(Stop::End(r)) => return Err(r),
            Err(Stop::Err(e)) => {
                tracing::warn!(call = %self.view.id, "call setup failed: {e}");
                return Err(end_for(&e));
            }
        }
        Ok(self.main_loop().await)
    }

    // --- incoming: ringing -----------------------------------------------------------

    async fn ring(&mut self) -> Result<(), EndReason> {
        let deadline = tokio::time::Instant::now() + RING_TIMEOUT;
        // First check right away: a push may announce a call that is already over.
        let mut poll_at = tokio::time::Instant::now();
        loop {
            tokio::select! {
                cmd = self.rx.recv() => match cmd {
                    None | Some(Cmd::HangUp) | Some(Cmd::Decline) => return Err(EndReason::Declined),
                    Some(Cmd::Accept) => {
                        // The caller may have given up a moment ago: joining then would
                        // start a new call and ring them back.
                        let c = self.client.clone();
                        let t = self.token();
                        if let Ok(Ok(room)) = tokio::time::timeout(REQUEST_TIMEOUT, async move { c.room(&t).await }).await {
                            if !room.has_call {
                                return Err(EndReason::Missed);
                            }
                        }
                        self.view.state = CallState::Connecting;
                        self.silent = true;
                        self.emit();
                        return Ok(());
                    }
                    Some(Cmd::Mute(m)) => { self.muted = m; self.view.muted = m; self.emit(); }
                    Some(Cmd::RecheckRing) => poll_at = tokio::time::Instant::now(),
                    Some(_) => {}
                },
                _ = sleep_until(poll_at) => {
                    poll_at = tokio::time::Instant::now() + RING_POLL;
                    match self.client.ring_state(&self.view.token).await {
                        Ok(RingState::Ringing) => {}
                        Ok(RingState::Missed) => return Err(EndReason::Missed),
                        Ok(RingState::Dismiss) => {
                            return Err(dismiss_reason(&self.client, &self.view.token, &self.me).await)
                        }
                        Err(e) => tracing::debug!(call = %self.view.id, "ring state: {e}"),
                    }
                }
                _ = sleep_until(deadline) => return Err(EndReason::Missed),
            }
        }
    }

    // --- setup -----------------------------------------------------------------------

    /// Waits for `fut` while still obeying hang-up and processing other commands.
    async fn abortable<T>(&mut self, fut: impl Future<Output = T>) -> Step<T> {
        tokio::pin!(fut);
        loop {
            tokio::select! {
                r = &mut fut => return Ok(r),
                cmd = self.rx.recv() => {
                    if let Some(r) = self.handle_cmd(cmd).await {
                        return Err(Stop::End(r));
                    }
                }
            }
        }
    }

    fn next_id(&mut self) -> String {
        self.msg_seq += 1;
        self.msg_seq.to_string()
    }

    /// Sends a request over the websocket and waits for the answer with the same id,
    /// handling everything else that arrives meanwhile.
    async fn request(&mut self, mut msg: Value) -> Step<Value> {
        let id = self.next_id();
        msg["id"] = Value::String(id.clone());
        let ws = self.ws.as_mut().ok_or_else(|| Stop::Err(signaling_err("no connection")))?;
        ws::send_text(ws, msg.to_string()).await?;
        let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;
        loop {
            let Some(ws) = self.ws.as_mut() else {
                return Err(Stop::Err(signaling_err("connection lost")));
            };
            tokio::select! {
                inc = ws::next(ws) => match inc {
                    Incoming::Text(t) => {
                        self.last_rx = Instant::now();
                        let Ok(v) = serde_json::from_str::<Value>(&t) else { continue };
                        if v.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                            if v.get("type").and_then(Value::as_str) == Some("error") {
                                let code = v["error"]["code"].as_str().unwrap_or("error").to_string();
                                let message = v["error"]["message"].as_str().unwrap_or_default().to_string();
                                return Err(Stop::Err(Error::Call { code, message }));
                            }
                            return Ok(v);
                        }
                        self.handle_message(v).await;
                    }
                    Incoming::Alive => self.last_rx = Instant::now(),
                    Incoming::Closed(r) => {
                        self.ws = None;
                        return Err(Stop::Err(signaling_err(&format!("closed: {r}"))));
                    }
                },
                cmd = self.rx.recv() => {
                    if let Some(r) = self.handle_cmd(cmd).await {
                        return Err(Stop::End(r));
                    }
                }
                _ = sleep_until(deadline) => return Err(Stop::Err(signaling_err("request timed out"))),
            }
        }
    }

    /// Opens the websocket and reads the `welcome` message: server features.
    async fn open_ws(&mut self, url: &str) -> Step<Vec<String>> {
        let client = self.client.clone();
        let u = url.to_string();
        let mut ws = self.abortable(async move { client.websocket(&u).await }).await??;
        let welcome = self
            .abortable(async {
                tokio::time::timeout(REQUEST_TIMEOUT, async {
                    loop {
                        match ws::next(&mut ws).await {
                            Incoming::Text(t) => {
                                let v: Value = serde_json::from_str(&t).unwrap_or_default();
                                if v["type"] == "welcome" {
                                    return Ok(v);
                                }
                            }
                            Incoming::Alive => {}
                            Incoming::Closed(r) => return Err(signaling_err(&format!("closed: {r}"))),
                        }
                    }
                })
                .await
                .map_err(|_| signaling_err("no welcome"))?
            })
            .await??;
        let features = welcome["welcome"]["features"]
            .as_array()
            .map(|a| a.iter().filter_map(|f| f.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        self.ws = Some(ws);
        self.last_rx = Instant::now();
        self.ping_sent = None;
        self.next_ping = Instant::now() + PING_INTERVAL;
        Ok(features)
    }

    async fn fetch_settings(&mut self) -> Step<SignalingSettings> {
        let client = self.client.clone();
        let token = self.token();
        let settings = self
            .abortable(async move { client.signaling_settings(Some(&token)).await })
            .await??;
        if settings.signaling_mode != "external" {
            return Err(Stop::Err(Error::Call {
                code: "no-hpb".into(),
                message: "Der Server hat kein High Performance Backend".into(),
            }));
        }
        Ok(settings)
    }

    fn ice_servers(&self) -> Vec<IceServer> {
        self.settings
            .as_ref()
            .map(|s| s.stunservers.iter().chain(s.turnservers.iter()).cloned().collect())
            .unwrap_or_default()
    }

    /// New signaling session, room and call. `first`: initial setup (checks, ringing).
    async fn connect_full(&mut self, first: bool) -> Step<()> {
        if first {
            let client = self.client.clone();
            if let Ok(caps) = self.abortable(async move { client.capabilities().await }).await? {
                let call = &caps["capabilities"]["spreed"]["config"]["call"];
                if call["end-to-end-encryption"].as_bool() == Some(true) {
                    return Err(Stop::End(failed(
                        "e2ee-required",
                        "Der Server verlangt verschlüsselte Anrufe – das kann diese App noch nicht",
                    )));
                }
                if call["enabled"].as_bool() == Some(false) {
                    return Err(Stop::End(failed("disabled", "Anrufe sind auf dem Server ausgeschaltet")));
                }
            }
            self.fetch_names();
        }
        let mut settings = self.fetch_settings().await?;
        let url = proto::signaling_url(&settings.server)
            .ok_or_else(|| Stop::Err(signaling_err("no signaling server in settings")))?;
        let backend = format!(
            "{}/ocs/v2.php/apps/spreed/api/v3/signaling/backend",
            self.client.account().server
        );

        let features = self.open_ws(&url).await?;
        self.mcu = features.iter().any(|f| f == "mcu");
        let v2 = features.iter().any(|f| f == "hello-v2");
        let hello = match settings.hello_auth_params["2.0"]["token"].as_str() {
            Some(t) if v2 => proto::hello_v2("", &backend, t),
            _ => proto::hello_v1(
                "",
                &backend,
                settings.user_id.as_deref().unwrap_or(&self.me),
                &settings.ticket,
            ),
        };
        let resp = match self.request(hello).await {
            Ok(r) => r,
            // The v2 token lives 60 s; fall back to the ticket instead of failing.
            Err(Stop::Err(Error::Call { code, .. })) if code.starts_with("token_") || code == "invalid_token" => {
                settings = self.fetch_settings().await?;
                let user = settings.user_id.clone().unwrap_or_else(|| self.me.clone());
                self.request(proto::hello_v1("", &backend, &user, &settings.ticket)).await?
            }
            Err(e) => return Err(e),
        };
        self.session_id = resp["hello"]["sessionid"].as_str().unwrap_or_default().to_string();
        self.resume_id = resp["hello"]["resumeid"].as_str().unwrap_or_default().to_string();
        self.settings = Some(settings);
        tracing::info!(call = %self.view.id, session = %self.session_id, mcu = self.mcu, "signaling connected");

        self.join_room_and_call().await?;
        if self.mcu {
            self.new_publisher();
        }
        Ok(())
    }

    async fn join_room_and_call(&mut self) -> Step<()> {
        for attempt in 0..2 {
            let client = self.client.clone();
            let token = self.token();
            self.nc_session = self.abortable(async move { client.join_room(&token, true).await }).await??;
            let msg = proto::join_room("", &self.view.token, &self.nc_session);
            self.request(msg).await?;
            let client = self.client.clone();
            let token = self.token();
            let silent = self.silent;
            match self.abortable(async move { client.join_call(&token, 3, silent).await }).await? {
                Ok(()) => break,
                // The Nextcloud session got lost (cookie, expiry): join the room again.
                Err(Error::NotFound | Error::Http { status: 404, .. }) if attempt == 0 => continue,
                Err(e) => return Err(Stop::Err(e)),
            }
        }
        // After the first join a rejoin must never ring anybody again.
        self.silent = true;
        if self.in_call_since.is_none() {
            self.in_call_since = Some(Instant::now());
        }
        if self.view.direction == CallDirection::Outgoing && self.view.state == CallState::Dialing {
            self.view.state = if self.others.values().any(|o| o.user.in_call & 1 != 0) {
                CallState::Connecting
            } else {
                CallState::Ringing
            };
        }
        self.emit();
        Ok(())
    }

    // --- main loop -------------------------------------------------------------------

    async fn main_loop(&mut self) -> EndReason {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            if let Some(r) = self.ended.take() {
                return r;
            }
            tokio::select! {
                inc = async {
                    match self.ws.as_mut() {
                        Some(ws) => ws::next(ws).await,
                        None => std::future::pending().await,
                    }
                } => self.on_ws(inc).await,
                cmd = self.rx.recv() => {
                    if let Some(r) = self.handle_cmd(cmd).await {
                        return r;
                    }
                }
                _ = tick.tick() => {
                    if let Err(r) = self.on_tick().await {
                        return r;
                    }
                }
            }
            self.emit();
        }
    }

    async fn on_ws(&mut self, inc: Incoming) {
        match inc {
            Incoming::Text(t) => {
                self.last_rx = Instant::now();
                match serde_json::from_str::<Value>(&t) {
                    Ok(v) => self.handle_message(v).await,
                    Err(e) => tracing::debug!("signaling: bad json: {e}"),
                }
            }
            Incoming::Alive => self.last_rx = Instant::now(),
            Incoming::Closed(r) => self.lost(&r),
        }
    }

    fn lost(&mut self, why: &str) {
        tracing::warn!(call = %self.view.id, "signaling lost: {why}");
        self.ws = None;
        if self.reconnect.is_none() {
            let resume_state = match self.view.state {
                CallState::Reconnecting => CallState::Connected,
                s => s,
            };
            let now = Instant::now();
            self.reconnect = Some(Reconnect {
                since: now,
                attempt: 0,
                next_at: now,
                resume_state,
            });
            if matches!(self.view.state, CallState::Connected | CallState::Connecting) {
                self.view.state = CallState::Reconnecting;
            }
        }
    }

    async fn on_tick(&mut self) -> Result<(), EndReason> {
        let now = Instant::now();
        if let Some(rc) = &self.reconnect {
            if rc.since.elapsed() > RECONNECT_GIVE_UP {
                return Err(failed("network", "Verbindung verloren"));
            }
            if now >= rc.next_at {
                self.try_reconnect().await?;
            }
            return Ok(());
        }

        // Keepalive: our own ping detects half-open TCP after network switches.
        if let Some((sent, timeout)) = self.ping_sent {
            if self.last_rx >= sent {
                self.ping_sent = None;
            } else if sent.elapsed() > timeout {
                self.lost("ping timeout");
                return Ok(());
            }
        } else if now >= self.next_ping {
            self.ping(PONG_TIMEOUT).await;
        }

        // Outgoing: nobody answered.
        if self.view.direction == CallDirection::Outgoing
            && !self.ever_joined
            && self.in_call_since.is_some_and(|t| t.elapsed() > RING_TIMEOUT)
        {
            return Err(EndReason::NoAnswer);
        }

        if self.mcu {
            self.check_publisher().await;
            self.check_subscribers().await;
        } else {
            self.check_mesh().await;
        }
        if !self.vanished.is_empty() {
            self.check_alone();
        }
        self.update_state();
        if self.names_fetched.is_some_and(|t| t.elapsed() > NAMES_REFRESH)
            && self.others.values().any(|o| o.name.is_empty())
        {
            self.fetch_names();
        }
        Ok(())
    }

    async fn ping(&mut self, timeout: Duration) {
        if let Some(ws) = self.ws.as_mut() {
            if ws::send_ping(ws).await.is_err() {
                self.lost("ping failed");
                return;
            }
            self.ping_sent = Some((Instant::now(), timeout));
            self.next_ping = Instant::now() + PING_INTERVAL;
        }
    }

    async fn try_reconnect(&mut self) -> Result<(), EndReason> {
        let Some(rc) = self.reconnect.as_ref() else { return Ok(()) };
        let since = rc.since;
        let resume_state = rc.resume_state;
        let result = if !self.resume_id.is_empty() && since.elapsed() < RESUME_WINDOW {
            match self.resume().await {
                Ok(true) => Ok(()),
                Ok(false) => self.rejoin().await,
                Err(e) => Err(e),
            }
        } else {
            self.rejoin().await
        };
        match result {
            Ok(()) => {
                tracing::info!(call = %self.view.id, after_ms = since.elapsed().as_millis() as u64, "reconnected");
                self.reconnect = None;
                if self.view.state == CallState::Reconnecting {
                    self.view.state = resume_state;
                }
                let queued = std::mem::take(&mut self.queued);
                for m in queued {
                    self.send(m).await;
                }
                Ok(())
            }
            Err(Stop::End(r)) => Err(r),
            Err(Stop::Err(e)) => {
                if let Error::Unauthorized = e {
                    return Err(end_for(&e));
                }
                // Permanent problems (permissions, call ended meanwhile) end the call.
                if let Error::Http { status, .. } = e {
                    if matches!(status, 400 | 403 | 412) {
                        return Err(end_for(&e));
                    }
                }
                self.ws = None;
                if let Some(rc) = self.reconnect.as_mut() {
                    rc.attempt += 1;
                    rc.next_at = Instant::now() + backoff(rc.attempt);
                    tracing::debug!(call = %self.view.id, attempt = rc.attempt, "reconnect failed: {e}");
                }
                Ok(())
            }
        }
    }

    /// Resumes the signaling session. `Ok(false)`: the session is gone, rejoin.
    async fn resume(&mut self) -> Step<bool> {
        let url = self
            .settings
            .as_ref()
            .and_then(|s| proto::signaling_url(&s.server))
            .ok_or_else(|| Stop::Err(signaling_err("no settings")))?;
        self.open_ws(&url).await?;
        let msg = proto::hello_resume("", &self.resume_id);
        match self.request(msg).await {
            Ok(resp) => {
                let sid = resp["hello"]["sessionid"].as_str().unwrap_or_default();
                if sid == self.session_id {
                    if let Some(r) = resp["hello"]["resumeid"].as_str() {
                        self.resume_id = r.to_string();
                    }
                    tracing::info!(call = %self.view.id, "signaling session resumed");
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Err(Stop::Err(Error::Call { code, .. })) if code == "no_such_session" => {
                self.resume_id.clear();
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// Everything new: signaling session, room, call and all peers.
    async fn rejoin(&mut self) -> Step<()> {
        tracing::info!(call = %self.view.id, "full rejoin");
        if let Some(ws) = self.ws.as_mut() {
            let _ = ws::send_text(ws, proto::bye().to_string()).await;
        }
        self.ws = None;
        self.close_all_peers();
        self.queued.clear();
        self.resume_id.clear();
        self.session_id.clear();
        self.connect_full(false).await
    }

    // --- commands --------------------------------------------------------------------

    async fn handle_cmd(&mut self, cmd: Option<Cmd>) -> Option<EndReason> {
        match cmd? {
            Cmd::HangUp => return Some(EndReason::Hangup),
            Cmd::Decline => {
                if self.view.state == CallState::Incoming {
                    return Some(EndReason::Declined);
                }
            }
            Cmd::Accept | Cmd::RecheckRing => {}
            Cmd::Mute(m) => {
                self.muted = m;
                self.view.muted = m;
                self.media.set_microphone_enabled(!m);
                let targets: Vec<String> = self.others.keys().cloned().collect();
                for t in targets {
                    self.send(proto::mute(&t, m)).await;
                }
            }
            Cmd::Media(r) => self.handle_media(r).await,
            Cmd::NetworkChanged => {
                tracing::info!(call = %self.view.id, "network changed");
                if let Some(rc) = self.reconnect.as_mut() {
                    rc.next_at = Instant::now();
                } else if self.ws.is_some() {
                    self.ping(PONG_TIMEOUT_PROBE).await;
                }
                if let Some(p) = &mut self.publisher {
                    if p.state != PeerState::Connected && p.restart_at.is_none() {
                        p.bad_since = Some(Instant::now() - DISCONNECTED_GRACE);
                    }
                }
            }
            Cmd::Names(names) => {
                self.names.extend(names);
                if let Some(n) = self.names.get(&self.me) {
                    self.nick = n.clone();
                }
                self.refresh_names();
            }
        }
        None
    }

    fn fetch_names(&mut self) {
        self.names_fetched = Some(Instant::now());
        let client = self.client.clone();
        let token = self.token();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Ok(list) = client.participants(&token).await {
                let names = list
                    .iter()
                    .filter_map(|p| {
                        Some((
                            p.get("actorId")?.as_str()?.to_string(),
                            p.get("displayName")?.as_str()?.to_string(),
                        ))
                    })
                    .collect();
                let _ = tx.send(Cmd::Names(names));
            }
        });
    }

    fn refresh_names(&mut self) {
        for (sid, o) in self.others.iter_mut() {
            if let Some(n) = self.session_names.get(sid).or_else(|| self.names.get(&o.user.actor_id)) {
                if !n.is_empty() {
                    o.name = n.clone();
                }
            }
        }
    }

    // --- signaling messages ----------------------------------------------------------

    async fn send(&mut self, msg: Value) {
        tracing::trace!(call = %self.view.id, "signaling out: {}", msg.to_string().chars().take(300).collect::<String>());
        if self.reconnect.is_some() || self.ws.is_none() {
            if self.queued.len() < 500 {
                self.queued.push(msg);
            }
            return;
        }
        let ws = self.ws.as_mut().unwrap();
        if let Err(e) = ws::send_text(ws, msg.to_string()).await {
            self.queued.push(msg);
            self.lost(&e.to_string());
        }
    }

    async fn handle_message(&mut self, v: Value) {
        tracing::trace!(call = %self.view.id, "signaling in: {}", v.to_string().chars().take(600).collect::<String>());
        match v["type"].as_str().unwrap_or_default() {
            "event" => self.handle_event(&v["event"]).await,
            "message" => {
                let sender = v["message"]["sender"]["sessionid"].as_str().unwrap_or_default().to_string();
                self.handle_peer_message(&sender, &v["message"]["data"]).await;
            }
            "control" => {
                if v["control"]["data"]["action"] == "forceMute" {
                    tracing::info!(call = %self.view.id, "muted by moderator");
                    self.muted = true;
                    self.view.muted = true;
                    self.media.set_microphone_enabled(false);
                }
            }
            "bye" => {
                self.resume_id.clear();
                self.lost(&format!("bye: {}", v["bye"]["reason"].as_str().unwrap_or_default()));
            }
            "error" => {
                let code = v["error"]["code"].as_str().unwrap_or_default();
                // A failed subscription request: the publisher was not ready yet.
                if let Some(session) = v["id"].as_str().and_then(|id| self.offer_requests.remove(id)) {
                    tracing::debug!(call = %self.view.id, session = %session, code, "requestoffer failed, retrying");
                    if let Some(o) = self.others.get_mut(&session) {
                        if o.peer.is_none() {
                            o.requested_at = Some(Instant::now() - OFFER_RETRY + OFFER_RETRY_AFTER_ERROR);
                        }
                    }
                    return;
                }
                tracing::warn!(call = %self.view.id, code, message = %v["error"]["message"], details = %v["error"]["details"], id = %v["id"], "signaling error");
                if matches!(code, "no_such_session" | "not_in_room" | "hello_expected") {
                    self.resume_id.clear();
                    self.lost(code);
                }
            }
            _ => {}
        }
    }

    async fn handle_event(&mut self, ev: &Value) {
        match (ev["target"].as_str(), ev["type"].as_str()) {
            (Some("room"), Some("join")) => {
                for j in ev["join"].as_array().into_iter().flatten() {
                    if let (Some(s), Some(n)) = (j["sessionid"].as_str(), j["user"]["displayname"].as_str()) {
                        self.session_names.insert(s.to_string(), n.to_string());
                    }
                }
                self.refresh_names();
            }
            (Some("room"), Some("leave")) => {
                let gone: Vec<String> = ev["leave"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect();
                for s in gone {
                    self.remove_other(&s, false);
                }
                self.check_alone();
            }
            (Some("participants"), Some("update")) => {
                let u = &ev["update"];
                if u["all"].as_bool() == Some(true) && u["incall"].as_u64() == Some(0) {
                    self.ended = Some(EndReason::RemoteHangup);
                    return;
                }
                if u.get("users").is_some() {
                    self.apply_users(proto::call_users(&u["users"]), true).await;
                } else if u.get("changed").is_some() {
                    self.apply_users(proto::call_users(&u["changed"]), false).await;
                }
            }
            _ => {}
        }
    }

    /// `full`: the list contains every session of the room (missing ones are gone).
    async fn apply_users(&mut self, users: Vec<CallUser>, full: bool) {
        let mut present = Vec::new();
        for u in users {
            if u.session_id == self.session_id {
                // We were removed from the call (moderator, server).
                if u.in_call & 1 == 0 && self.in_call_since.is_some_and(|t| t.elapsed() > Duration::from_secs(3)) {
                    tracing::info!(call = %self.view.id, "removed from call by server");
                    self.ended = Some(EndReason::RemoteHangup);
                }
                continue;
            }
            // Internal sessions (recording, SIP bridge) carry no audio for us. Our own
            // other sessions (another device, a stale session of a crashed app) are not
            // somebody we talk to.
            if u.internal || u.session_id.is_empty() || u.actor_type == "users" && u.actor_id == self.me {
                continue;
            }
            if u.in_call & 1 == 0 {
                self.remove_other(&u.session_id, true);
                continue;
            }
            present.push(u.session_id.clone());
            let muted_now = self.muted;
            match self.others.get_mut(&u.session_id) {
                Some(o) => o.user = u,
                None => {
                    tracing::info!(call = %self.view.id, session = %u.session_id, actor = %u.actor_id, "participant joined");
                    self.vanished.remove(&u.actor_id);
                    let sid = u.session_id.clone();
                    let name = self
                        .session_names
                        .get(&sid)
                        .or_else(|| self.names.get(&u.actor_id))
                        .cloned()
                        .unwrap_or_default();
                    self.others.insert(
                        sid.clone(),
                        Other {
                            user: u,
                            name,
                            muted: false,
                            speaking_until: None,
                            connected: false,
                            peer: None,
                            requested_at: None,
                            not_before: self
                                .in_call_since
                                .filter(|t| t.elapsed() > Duration::from_secs(1))
                                .map(|_| Instant::now() + NEWCOMER_SUBSCRIBE_DELAY),
                        },
                    );
                    self.ever_joined = true;
                    if self.someone_since.is_none() {
                        self.someone_since = Some(Instant::now());
                    }
                    // Tell the newcomer that we are muted.
                    if muted_now {
                        self.send(proto::mute(&sid, true)).await;
                    }
                }
            }
        }
        if full {
            let gone: Vec<String> = self.others.keys().filter(|k| !present.contains(k)).cloned().collect();
            for s in gone {
                self.remove_other(&s, false);
            }
        }
        self.check_alone();
        if self.others.values().any(|o| o.name.is_empty())
            && self.names_fetched.is_none_or(|t| t.elapsed() > Duration::from_secs(2))
        {
            self.fetch_names();
        }
        if self.mcu {
            self.check_subscribers().await;
        } else {
            self.check_mesh().await;
        }
    }

    /// `hung_up`: the session explicitly left the call (inCall 0). Otherwise it vanished
    /// (lost connection, rejoining with a new session) and may come back.
    fn remove_other(&mut self, session: &str, hung_up: bool) {
        if let Some(o) = self.others.remove(session) {
            tracing::info!(call = %self.view.id, session, hung_up, "participant left");
            if !hung_up && !o.user.actor_id.is_empty() {
                self.vanished.insert(o.user.actor_id.clone(), Instant::now());
            } else {
                self.vanished.remove(&o.user.actor_id);
            }
            if let Some(p) = o.peer {
                self.close_peer(&p.peer_id);
            }
        }
    }

    /// 1:1: the other side left → the call is over (like a phone call). A vanished
    /// session gets [`VANISH_GRACE`] to come back.
    fn check_alone(&mut self) {
        self.vanished.retain(|_, t| t.elapsed() < VANISH_GRACE);
        if !self.view.is_group
            && self.ever_joined
            && self.others.is_empty()
            && self.vanished.is_empty()
            && self.ended.is_none()
        {
            self.ended = Some(EndReason::RemoteHangup);
        }
        if self.others.is_empty() {
            self.someone_since = None;
        }
    }

    async fn handle_peer_message(&mut self, sender: &str, data: &Value) {
        let t = data["type"].as_str().unwrap_or_default();
        let sid = data["sid"].as_str().unwrap_or_default().to_string();
        let payload = &data["payload"];
        let own = sender == self.session_id;
        match t {
            "offer" if !own => {
                let Some(sdp) = payload["sdp"].as_str() else { return };
                self.remote_offer(sender, &sid, sdp.to_string());
            }
            "answer" => {
                let Some(sdp) = payload["sdp"].as_str() else { return };
                tracing::trace!(call = %self.view.id, "remote answer sdp: {}", sdp.replace("\r\n", " | "));
                if own || self.mcu {
                    if let Some(p) = self.publisher.as_mut().filter(|p| p.sid == sid || sid.is_empty()) {
                        p.answered = true;
                        let id = p.peer_id.clone();
                        let early = std::mem::take(&mut p.early_candidates);
                        let psid = p.sid.clone();
                        self.media.set_remote_description(id, SdpType::Answer, sdp.to_string());
                        let own_session = self.session_id.clone();
                        for c in early {
                            self.send(proto::candidate(&own_session, &psid, &c)).await;
                        }
                    }
                } else if let Some(peer) = self.others.get(sender).and_then(|o| o.peer.as_ref()) {
                    if peer.sid == sid && peer.offerer {
                        self.media.set_remote_description(peer.peer_id.clone(), SdpType::Answer, sdp.to_string());
                    }
                }
            }
            "candidate" => {
                let Some(c) = proto::candidate_from(payload) else { return };
                if own {
                    if let Some(p) = self.publisher.as_ref().filter(|p| p.sid == sid || sid.is_empty()) {
                        self.media.add_remote_candidate(p.peer_id.clone(), c);
                    }
                } else if let Some(peer) = self.others.get(sender).and_then(|o| o.peer.as_ref()) {
                    if peer.sid == sid || sid.is_empty() {
                        self.media.add_remote_candidate(peer.peer_id.clone(), c);
                    }
                }
            }
            "mute" | "unmute" => {
                if payload["name"] == "audio" {
                    if let Some(o) = self.others.get_mut(sender) {
                        o.muted = t == "mute";
                        if o.muted {
                            o.speaking_until = None;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn remote_offer(&mut self, sender: &str, sid: &str, sdp: String) {
        let mcu = self.mcu;
        let my_session = self.session_id.clone();
        let ice = self.ice_servers();
        if !self.others.contains_key(sender) {
            // Offers may overtake the participants update.
            self.others.insert(
                sender.to_string(),
                Other {
                    user: CallUser {
                        session_id: sender.to_string(),
                        in_call: 3,
                        actor_id: String::new(),
                        actor_type: String::new(),
                        internal: false,
                    },
                    name: self.session_names.get(sender).cloned().unwrap_or_default(),
                    muted: false,
                    speaking_until: None,
                    connected: false,
                    peer: None,
                    requested_at: None,
                    not_before: None,
                },
            );
        }
        self.offer_requests.retain(|_, s| s != sender);
        let other = self.others.get_mut(sender).unwrap();
        // Renegotiation of a working subscription.
        if let Some(p) = other.peer.as_ref() {
            let alive = !matches!(p.state, PeerState::Failed | PeerState::Closed);
            if p.sid == sid && alive && (mcu || !p.offerer) {
                let id = p.peer_id.clone();
                self.media.set_remote_description(id.clone(), SdpType::Offer, sdp);
                self.media.create_answer(id);
                return;
            }
            // Mesh glare: both offered. The bigger session id keeps its offer.
            if !mcu && p.offerer && my_session.as_str() > sender && alive {
                tracing::debug!(call = %self.view.id, "mesh glare: keeping own offer");
                return;
            }
        }
        let old = other.peer.take().map(|p| p.peer_id);
        self.peer_seq += 1;
        let peer_id = format!("{}-{}", if mcu { "sub" } else { "mesh" }, self.peer_seq);
        let other = self.others.get_mut(sender).unwrap();
        other.peer = Some(Peer {
            peer_id: peer_id.clone(),
            sid: sid.to_string(),
            state: PeerState::New,
            created: Instant::now(),
            bad_since: None,
            offerer: false,
        });
        other.connected = false;
        if let Some(old) = old {
            self.close_peer(&old);
        }
        self.peer_session.insert(peer_id.clone(), sender.to_string());
        let kind = if mcu { PeerKind::Subscriber } else { PeerKind::Mesh };
        self.media.create_peer(peer_id.clone(), kind, ice);
        if !mcu {
            self.media.set_microphone_enabled(!self.muted);
        }
        self.media.set_remote_description(peer_id.clone(), SdpType::Offer, sdp);
        self.media.create_answer(peer_id);
    }

    // --- media -----------------------------------------------------------------------

    async fn handle_media(&mut self, r: MediaReport) {
        match r {
            MediaReport::LocalDescription { peer_id, sdp_type, sdp } => {
                if self.owners.get(&peer_id) == Some(&Owner::Publisher) {
                    let Some(p) = self.publisher.as_mut().filter(|p| p.peer_id == peer_id) else { return };
                    p.offer_sent_at = Some(Instant::now());
                    let (sid, own) = (p.sid.clone(), self.session_id.clone());
                    let msg = proto::sdp(&own, &sid, sdp_type, &sdp, &self.nick);
                    tracing::debug!(call = %self.view.id, "publisher offer: {}", msg);
                    self.send(msg).await;
                } else if let Some(session) = self.peer_session.get(&peer_id).cloned() {
                    let Some(peer) = self.others.get(&session).and_then(|o| o.peer.as_ref()) else { return };
                    if peer.peer_id != peer_id {
                        return;
                    }
                    let msg = proto::sdp(&session, &peer.sid, sdp_type, &sdp, &self.nick);
                    self.send(msg).await;
                }
            }
            MediaReport::LocalCandidate { peer_id, candidate } => {
                if self.owners.get(&peer_id) == Some(&Owner::Publisher) {
                    let Some(p) = self.publisher.as_mut().filter(|p| p.peer_id == peer_id) else { return };
                    if !p.answered {
                        p.early_candidates.push(candidate);
                        return;
                    }
                    let msg = proto::candidate(&self.session_id, &p.sid, &candidate);
                    self.send(msg).await;
                } else if let Some(session) = self.peer_session.get(&peer_id).cloned() {
                    let Some(peer) = self.others.get(&session).and_then(|o| o.peer.as_ref()) else { return };
                    if peer.peer_id == peer_id {
                        let msg = proto::candidate(&session, &peer.sid, &candidate);
                        self.send(msg).await;
                    }
                }
            }
            MediaReport::PeerState { peer_id, state } => self.peer_state(&peer_id, state),
            MediaReport::PeerError { peer_id, message } => {
                tracing::warn!(call = %self.view.id, peer = %peer_id, "media error: {message}");
                self.peer_state(&peer_id, PeerState::Failed);
            }
            MediaReport::AudioLevel { peer_id, level } => {
                if let Some(session) = self.peer_session.get(&peer_id) {
                    if let Some(o) = self.others.get_mut(session) {
                        if level >= SPEAKING_LEVEL && !o.muted {
                            o.speaking_until = Some(Instant::now() + SPEAKING_HOLD);
                        }
                    }
                }
            }
        }
    }

    fn peer_state(&mut self, peer_id: &str, state: PeerState) {
        tracing::debug!(call = %self.view.id, peer = peer_id, ?state, "peer state");
        let bad = matches!(state, PeerState::Disconnected | PeerState::Failed);
        if let Some(p) = self.publisher.as_mut().filter(|p| p.peer_id == peer_id) {
            p.state = state;
            if state == PeerState::Connected {
                p.bad_since = None;
                p.restart_at = None;
                self.publisher_failures = 0;
            } else if bad && p.bad_since.is_none() {
                p.bad_since = Some(Instant::now());
            }
            return;
        }
        let Some(session) = self.peer_session.get(peer_id).cloned() else { return };
        let Some(o) = self.others.get_mut(&session) else { return };
        let Some(peer) = o.peer.as_mut().filter(|p| p.peer_id == peer_id) else { return };
        peer.state = state;
        if state == PeerState::Connected {
            peer.bad_since = None;
            o.connected = true;
            o.requested_at = None;
        } else if bad {
            o.connected = false;
            if peer.bad_since.is_none() {
                peer.bad_since = Some(Instant::now());
            }
        }
    }

    fn new_publisher(&mut self) {
        if let Some(old) = self.publisher.take() {
            self.close_peer(&old.peer_id);
        }
        self.peer_seq += 1;
        let peer_id = format!("pub-{}", self.peer_seq);
        self.owners.insert(peer_id.clone(), Owner::Publisher);
        self.publisher = Some(Publisher {
            peer_id: peer_id.clone(),
            sid: random_id(),
            state: PeerState::New,
            created: Instant::now(),
            offer_sent_at: None,
            answered: false,
            bad_since: None,
            restart_at: None,
            early_candidates: Vec::new(),
        });
        tracing::info!(call = %self.view.id, peer = %peer_id, "publishing");
        self.media.create_peer(peer_id.clone(), PeerKind::Publisher, self.ice_servers());
        self.media.set_microphone_enabled(!self.muted);
        self.media.create_offer(peer_id, false);
    }

    async fn check_publisher(&mut self) {
        let Some(p) = self.publisher.as_mut() else {
            self.new_publisher();
            return;
        };
        let now = Instant::now();
        // Lost answer: the connection would stay `new` forever.
        if !p.answered && p.offer_sent_at.is_some_and(|t| now - t > PUBLISHER_ANSWER_TIMEOUT)
            || p.offer_sent_at.is_none() && now - p.created > PUBLISHER_ANSWER_TIMEOUT * 2
        {
            tracing::warn!(call = %self.view.id, "publisher got no answer, recreating");
            self.publisher_trouble();
            return;
        }
        if p.answered
            && p.bad_since.is_none()
            && matches!(p.state, PeerState::New | PeerState::Connecting)
            && p.offer_sent_at.is_some_and(|t| now - t > CONNECT_TIMEOUT)
            && p.restart_at.is_none()
        {
            tracing::warn!(call = %self.view.id, "publisher not connecting");
            p.bad_since = Some(now - DISCONNECTED_GRACE);
        }
        let Some(bad_since) = p.bad_since else { return };
        let grace = if p.state == PeerState::Failed { Duration::ZERO } else { DISCONNECTED_GRACE };
        match p.restart_at {
            None if now - bad_since >= grace => {
                tracing::info!(call = %self.view.id, "publisher ICE restart");
                p.restart_at = Some(now);
                let id = p.peer_id.clone();
                self.media.create_offer(id, true);
            }
            Some(t) if now - t > ICE_RESTART_TIMEOUT => {
                tracing::warn!(call = %self.view.id, "ICE restart did not help");
                self.publisher_trouble();
            }
            _ => {}
        }
    }

    /// The MCU keeps one publisher per session and stream: a replacement needs a new
    /// signaling session, so publisher trouble that an ICE restart did not fix means a
    /// full rejoin.
    fn publisher_trouble(&mut self) {
        self.publisher_failures += 1;
        self.resume_id.clear();
        self.lost("publisher not working");
    }

    async fn check_subscribers(&mut self) {
        let now = Instant::now();
        let mut requests = Vec::new();
        let mut closes = Vec::new();
        for (session, o) in self.others.iter_mut() {
            if !o.has_media() || o.not_before.is_some_and(|t| now < t) {
                continue;
            }
            if let Some(p) = &o.peer {
                let broken = p.state == PeerState::Failed
                    || p.state == PeerState::Closed
                    || p.bad_since.is_some_and(|t| now - t > DISCONNECTED_GRACE * 2)
                    || matches!(p.state, PeerState::New | PeerState::Connecting) && now - p.created > CONNECT_TIMEOUT;
                if broken {
                    tracing::info!(call = %self.view.id, session, "subscriber broken, resubscribing");
                    closes.push(p.peer_id.clone());
                    o.peer = None;
                    o.connected = false;
                    o.requested_at = None;
                } else {
                    continue;
                }
            }
            if o.requested_at.is_none_or(|t| now - t > OFFER_RETRY) {
                o.requested_at = Some(now);
                requests.push(session.clone());
            }
        }
        for id in closes {
            self.close_peer(&id);
        }
        for s in requests {
            let mut msg = proto::request_offer(&s, None);
            let id = format!("ro{}", self.next_id());
            msg["id"] = Value::String(id.clone());
            self.offer_requests.insert(id, s);
            self.send(msg).await;
        }
    }

    async fn check_mesh(&mut self) {
        let now = Instant::now();
        let mine = self.session_id.clone();
        let mut offers = Vec::new();
        let mut closes = Vec::new();
        for (session, o) in self.others.iter_mut() {
            if !o.has_media() {
                continue;
            }
            if let Some(p) = &o.peer {
                let broken = p.state == PeerState::Failed
                    || p.bad_since.is_some_and(|t| now - t > DISCONNECTED_GRACE * 2);
                if !broken {
                    continue;
                }
                closes.push(p.peer_id.clone());
                o.peer = None;
                o.connected = false;
                o.requested_at = Some(now);
            }
            let we_offer = mine.as_str() > session.as_str();
            let waited = o.requested_at.get_or_insert(now);
            if we_offer || now - *waited > MESH_OFFER_FALLBACK {
                offers.push(session.clone());
            }
        }
        for id in closes {
            self.close_peer(&id);
        }
        let ice = self.ice_servers();
        for s in offers {
            self.peer_seq += 1;
            let peer_id = format!("mesh-{}", self.peer_seq);
            let Some(o) = self.others.get_mut(&s) else { continue };
            o.peer = Some(Peer {
                peer_id: peer_id.clone(),
                sid: random_id(),
                state: PeerState::New,
                created: now,
                bad_since: None,
                offerer: true,
            });
            o.requested_at = None;
            self.peer_session.insert(peer_id.clone(), s);
            self.media.create_peer(peer_id.clone(), PeerKind::Mesh, ice.clone());
            self.media.set_microphone_enabled(!self.muted);
            self.media.create_offer(peer_id, false);
        }
    }

    fn close_peer(&mut self, peer_id: &str) {
        self.owners.remove(peer_id);
        self.peer_session.remove(peer_id);
        self.media.close_peer(peer_id.to_string());
    }

    fn close_all_peers(&mut self) {
        if let Some(p) = self.publisher.take() {
            self.close_peer(&p.peer_id);
        }
        let ids: Vec<String> = self
            .others
            .values_mut()
            .filter_map(|o| {
                o.connected = false;
                o.requested_at = None;
                o.peer.take().map(|p| p.peer_id)
            })
            .collect();
        for id in ids {
            self.close_peer(&id);
        }
    }

    /// Derives the shown state from participants and media.
    fn update_state(&mut self) {
        let now = Instant::now();
        let someone = !self.others.is_empty();
        let media_ok = self.others.values().any(|o| o.connected);
        match self.view.state {
            CallState::Ringing | CallState::Connecting | CallState::Dialing if someone => {
                if media_ok || self.someone_since.is_some_and(|t| now - t > CONNECTED_GRACE) {
                    self.view.state = CallState::Connected;
                    self.view.started_at = Some(now_ms());
                    tracing::info!(call = %self.view.id, "connected");
                } else if self.view.state != CallState::Connecting {
                    self.view.state = CallState::Connecting;
                }
            }
            CallState::Connected | CallState::Reconnecting if self.reconnect.is_none() => {
                // Media trouble (publisher down or nobody audible) shows as reconnecting.
                let pub_bad = self.publisher.as_ref().is_some_and(|p| p.bad_since.is_some());
                let subs_bad = someone && !media_ok && self.others.values().any(|o| o.has_media());
                if pub_bad || subs_bad {
                    let since = *self.media_trouble_since.get_or_insert(now);
                    if now - since > MEDIA_TROUBLE_SHOWN_AFTER {
                        self.view.state = CallState::Reconnecting;
                    }
                } else {
                    self.media_trouble_since = None;
                    self.view.state = CallState::Connected;
                }
                if self.view.state == CallState::Connected && self.view.started_at.is_none() {
                    self.view.started_at = Some(now_ms());
                }
            }
            _ => {}
        }
        if !self.view.is_group && !someone && !self.vanished.is_empty() && self.view.state == CallState::Connected {
            // The other side is reconnecting.
            self.view.state = CallState::Reconnecting;
        }
        if self.view.is_group && !someone && self.view.state == CallState::Connected {
            // Everybody else left a group call: waiting for others.
            self.view.state = CallState::Ringing;
        }
    }

    // --- view ------------------------------------------------------------------------

    fn emit(&mut self) {
        let now = Instant::now();
        let mut participants: Vec<CallParticipant> = self
            .others
            .iter()
            .map(|(sid, o)| CallParticipant {
                session_id: sid.clone(),
                user_id: if o.user.actor_type == "users" { o.user.actor_id.clone() } else { String::new() },
                name: o.name.clone(),
                muted: o.muted,
                speaking: !o.muted && o.speaking_until.is_some_and(|t| t > now),
                connected: o.connected,
            })
            .collect();
        participants.sort_by(|a, b| a.name.cmp(&b.name).then(a.session_id.cmp(&b.session_id)));
        self.view.participants = participants;
        if self.last_emitted.as_ref() == Some(&self.view) {
            return;
        }
        let v = self.view.clone();
        *self.shared.view.lock().unwrap() = Some(v.clone());
        self.last_emitted = Some(v.clone());
        self.sink.emit(Event::CallChanged { call: Some(v) });
    }

    async fn teardown(&mut self, reason: EndReason) {
        tracing::info!(call = %self.view.id, ?reason, "call ended");
        if reason == EndReason::Missed && self.view.direction == CallDirection::Incoming {
            self.sink.emit(Event::CallMissed {
                token: self.view.token.clone(),
                room_name: self.view.room_name.clone(),
                caller_name: self.view.caller_name.clone(),
                is_group: self.view.is_group,
            });
        }
        self.view.state = CallState::Ended;
        self.view.end_reason = Some(reason);
        self.emit();
        self.close_all_peers();
        if self.audio_started {
            self.media.stop_audio(self.view.id.clone());
        }
        if !self.nc_session.is_empty() {
            let token = self.token();
            let client = self.client.clone();
            // Leave the call first: the others see "left the call" (a hang-up) before the
            // session disappears, which they would treat as a connection loss.
            let _ = tokio::time::timeout(LEAVE_TIMEOUT, async {
                if let Err(e) = client.leave_call(&token, false).await {
                    tracing::debug!("leave call: {e}");
                }
            })
            .await;
            if let Some(ws) = self.ws.as_mut() {
                let _ = ws::send_text(ws, proto::leave_room("leave").to_string()).await;
                let _ = ws::send_text(ws, proto::bye().to_string()).await;
            }
            let _ = tokio::time::timeout(LEAVE_TIMEOUT, client.leave_room(&token)).await;
        }
        self.ws = None;
        *self.shared.tx.lock().unwrap() = None;
        *self.shared.view.lock().unwrap() = None;
        self.sink.emit(Event::CallChanged { call: None });
    }
}

/// Ringing stopped on the server side: answered on another device, or over.
async fn dismiss_reason(client: &TalkClient, token: &str, me: &str) -> EndReason {
    match client.call_peers(token).await {
        Ok(peers)
            if peers.iter().any(|p| {
                p.get("actorType").and_then(Value::as_str) == Some("users")
                    && p.get("actorId").and_then(Value::as_str) == Some(me)
            }) =>
        {
            EndReason::AnsweredElsewhere
        }
        _ => EndReason::Missed,
    }
}

/// Placeholder while ringing without a registered media engine (never receives commands:
/// audio starts only after `accept`, when the real engine is required).
struct NoMedia;

impl MediaEngine for NoMedia {
    fn create_peer(&self, _: String, _: PeerKind, _: Vec<IceServer>) {}
    fn create_offer(&self, _: String, _: bool) {}
    fn create_answer(&self, _: String) {}
    fn set_remote_description(&self, _: String, _: SdpType, _: String) {}
    fn add_remote_candidate(&self, _: String, _: super::IceCandidate) {}
    fn close_peer(&self, _: String) {}
    fn set_microphone_enabled(&self, _: bool) {}
    fn start_audio(&self, _: String) {}
    fn stop_audio(&self, _: String) {}
}

fn signaling_err(message: &str) -> Error {
    Error::Call {
        code: "signaling".into(),
        message: message.into(),
    }
}
