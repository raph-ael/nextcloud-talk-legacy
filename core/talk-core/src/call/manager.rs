//! Owner of the single active call: starts the call task and forwards commands to it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use super::actor::{Actor, Cmd, Ctx, Shared};
use super::slot::{CallSlot, Holder, Live};
use super::{CallDirection, CallRecording, CallState, CallView, MediaEngine, MediaReport};
use crate::client::TalkClient;
use crate::engine::{Event, EventSink};
use crate::error::{Error, Result};

/// The same incoming call (one notification) is not rung twice: it arrives from push,
/// notify_push and the room list, and a declined call keeps its notification. A new call
/// has a new notification id and rings at once (call-back right after a missed call).
const RING_DEDUP: Duration = Duration::from_secs(10 * 60);
/// Without a notification id (unknown source) only briefly.
const RING_DEDUP_UNKEYED: Duration = Duration::from_secs(30);

/// What the engine knows about a conversation when a call starts.
pub(crate) struct RoomInfo {
    pub token: String,
    pub name: String,
    pub is_group: bool,
    pub has_call: bool,
}

pub(crate) struct CallManager {
    media: Arc<RwLock<Option<Arc<dyn MediaEngine>>>>,
    shared: Arc<Shared>,
    /// "token#notification id" → when it rang here (see [`RING_DEDUP`]).
    rung: Mutex<HashMap<String, Instant>>,
    /// One call per process, across all accounts.
    slot: Arc<CallSlot>,
    /// This engine in the slot.
    owner: u64,
}

impl Default for CallManager {
    fn default() -> Self {
        Self::with_slot(CallSlot::global())
    }
}

/// What a call somewhere in this process means for a new incoming one here.
enum Busy {
    /// The very same call rings or runs under another account (two accounts of one
    /// server in the same conversation): stay silent, it is not missed.
    SameCall,
    Other,
}

fn call_error(code: &str, message: &str) -> Error {
    Error::Call {
        code: code.into(),
        message: message.into(),
    }
}

impl CallManager {
    pub(crate) fn set_media_engine(&self, media: Arc<dyn MediaEngine>) {
        *self.media.write().unwrap() = Some(media);
    }

    pub(crate) fn current(&self) -> Option<CallView> {
        self.shared.view.lock().unwrap().clone()
    }

    pub(crate) fn with_slot(slot: Arc<CallSlot>) -> Self {
        Self {
            media: Default::default(),
            shared: Default::default(),
            rung: Default::default(),
            slot,
            owner: CallSlot::next_owner(),
        }
    }

    /// This engine has a call (ringing counts).
    pub(crate) fn busy(&self) -> bool {
        self.shared.tx.lock().unwrap().is_some()
    }

    /// Some engine of this process has a call: a second one cannot start or ring.
    pub(crate) fn busy_anywhere(&self) -> bool {
        self.busy() || self.slot.holder().is_some()
    }

    /// The call in the slot is this engine's own call in `token`.
    pub(crate) fn holds(&self, token: &str) -> bool {
        self.slot.holder().is_some_and(|h| h.owner == self.owner && h.token == token)
    }

    fn busy_for(&self, client: &TalkClient, token: &str) -> Option<Busy> {
        let h = self.slot.holder()?;
        let same = h.owner != self.owner
            && h.token == token
            && h.server == crate::accounts::server_key(&client.account().server);
        Some(if same { Busy::SameCall } else { Busy::Other })
    }

    fn ring_key(token: &str, notification_id: i64) -> String {
        format!("{token}#{notification_id}")
    }

    /// This notification was handled here already (rang, was missed or was left to
    /// another account), see [`RING_DEDUP`].
    pub(crate) fn has_rung(&self, token: &str, notification_id: i64) -> bool {
        let mut rung = self.rung.lock().unwrap();
        rung.retain(|k, t| t.elapsed() < if k.ends_with("#0") { RING_DEDUP_UNKEYED } else { RING_DEDUP });
        rung.contains_key(&Self::ring_key(token, notification_id))
    }

    fn send(&self, cmd: Cmd) -> bool {
        match self.shared.tx.lock().unwrap().as_ref() {
            Some(tx) => tx.send(cmd).is_ok(),
            None => false,
        }
    }

    fn launch(
        &self,
        client: TalkClient,
        sink: Arc<dyn EventSink>,
        me: String,
        view: CallView,
        silent: bool,
        runtime: &tokio::runtime::Handle,
    ) -> Result<()> {
        // Ringing works without a media engine (it is only needed once audio starts).
        if view.state != CallState::Incoming && self.media.read().unwrap().is_none() {
            return Err(call_error("no-media-engine", "setMediaEngine was not called"));
        }
        let busy = || call_error("busy", "Es läuft bereits ein Anruf");
        let guard = self
            .slot
            .acquire(Holder {
                owner: self.owner,
                server: crate::accounts::server_key(&client.account().server),
                token: view.token.clone(),
            })
            .ok_or_else(busy)?;
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut slot = self.shared.tx.lock().unwrap();
            if slot.is_some() {
                return Err(busy());
            }
            *slot = Some(tx.clone());
        }
        *self.shared.view.lock().unwrap() = Some(view.clone());
        let ctx = Ctx {
            client,
            live: Live::new(self.shared.clone(), sink.clone(), guard),
            sink,
            media: self.media.clone(),
            shared: self.shared.clone(),
            me,
        };
        let actor = Actor::new(ctx, view, silent, rx, tx);
        runtime.spawn(actor.run());
        Ok(())
    }

    pub(crate) fn start(
        &self,
        client: TalkClient,
        sink: Arc<dyn EventSink>,
        me: String,
        room: RoomInfo,
        silent: bool,
        call_id: Option<String>,
        runtime: &tokio::runtime::Handle,
    ) -> Result<()> {
        let view = CallView {
            id: call_id.filter(|c| !c.is_empty()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            token: room.token,
            room_name: room.name,
            is_group: room.is_group,
            direction: CallDirection::Outgoing,
            // Joining a running call does not ring anybody.
            state: if room.has_call { CallState::Connecting } else { CallState::Dialing },
            caller_name: String::new(),
            started_at: None,
            muted: false,
            participants: Vec::new(),
            end_reason: None,
            recording: CallRecording::Off,
            can_record: false,
            recording_consent_pending: false,
            camera_on: false,
            screen_sharing: false,
            can_share_screen: false,
            screens: Vec::new(),
        };
        self.launch(client, sink, me, view, silent, runtime)
    }

    /// An incoming call was detected. Returns `false` when it does not ring: it rang or
    /// was declined here a moment ago, or a call is in progress in this process. Then it
    /// is reported as missed, unless it is the call another account is already in.
    pub(crate) fn ring(
        &self,
        client: TalkClient,
        sink: Arc<dyn EventSink>,
        me: String,
        room: RoomInfo,
        caller_name: String,
        notification_id: i64,
        call_id: Option<String>,
        runtime: &tokio::runtime::Handle,
    ) -> bool {
        {
            if self.has_rung(&room.token, notification_id) {
                return false;
            }
            self.rung
                .lock()
                .unwrap()
                .insert(Self::ring_key(&room.token, notification_id), Instant::now());
            let busy = match self.busy_for(&client, &room.token) {
                None if self.busy() => Some(Busy::Other),
                b => b,
            };
            if let Some(Busy::SameCall) = busy {
                tracing::info!(token = %room.token, "incoming call rings under another account already");
                return false;
            }
            if busy.is_some() {
                // In a call already: no second ring, but tell the user (like Signal's
                // "missed call" while busy).
                tracing::info!(token = %room.token, "incoming call while busy: missed");
                sink.emit(Event::CallMissed {
                    token: room.token,
                    room_name: room.name,
                    caller_name,
                    is_group: room.is_group,
                });
                return false;
            }
        }
        let view = CallView {
            id: call_id.filter(|c| !c.is_empty()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            token: room.token,
            room_name: room.name,
            is_group: room.is_group,
            direction: CallDirection::Incoming,
            state: CallState::Incoming,
            caller_name,
            started_at: None,
            muted: false,
            participants: Vec::new(),
            end_reason: None,
            recording: CallRecording::Off,
            can_record: false,
            recording_consent_pending: false,
            camera_on: false,
            screen_sharing: false,
            can_share_screen: false,
            screens: Vec::new(),
        };
        self.launch(client, sink, me, view, true, runtime).is_ok()
    }

    pub(crate) fn accept(&self, token: &str) -> Result<()> {
        let ok = self
            .current()
            .is_some_and(|v| v.token == token && v.state == CallState::Incoming);
        if !ok || !self.send(Cmd::Accept) {
            return Err(call_error("no-incoming-call", "Kein eingehender Anruf"));
        }
        Ok(())
    }

    pub(crate) fn decline(&self, token: &str) {
        if self.current().is_some_and(|v| v.token == token) {
            self.send(Cmd::Decline);
        }
    }

    pub(crate) fn hang_up(&self) {
        self.send(Cmd::HangUp);
    }

    /// Hangs up and waits until the server knows we left (or `timeout` passed).
    pub(crate) async fn hang_up_and_wait(&self, timeout: Duration) {
        if !self.send(Cmd::HangUp) {
            return;
        }
        let deadline = Instant::now() + timeout;
        while self.busy() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub(crate) fn set_muted(&self, muted: bool) {
        self.send(Cmd::Mute(muted));
    }

    pub(crate) fn set_camera(&self, on: bool) {
        self.send(Cmd::Camera(on));
    }

    pub(crate) fn set_screen_share(&self, on: bool) {
        self.send(Cmd::Screen(on));
    }

    pub(crate) fn report(&self, report: MediaReport) {
        self.send(Cmd::Media(report));
    }

    /// Something changed about ringing (delete push): check the ring state now.
    pub(crate) fn recheck_ring(&self) {
        self.send(Cmd::RecheckRing);
    }

    pub(crate) fn network_changed(&self) {
        self.send(Cmd::NetworkChanged);
    }

    pub(crate) fn consent_to_recording(&self) {
        self.send(Cmd::ConsentToRecording);
    }

    pub(crate) fn recording_requested(&self, r: CallRecording) {
        self.send(Cmd::RecordingRequested(r));
    }

    pub(crate) fn force_rejoin(&self) {
        self.send(Cmd::ForceRejoin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Account, Platform};

    /// Nothing listens there: every request fails at once, a ringing call keeps ringing.
    const SERVER: &str = "http://127.0.0.1:9";
    const OTHER_SERVER: &str = "http://127.0.0.1:10";

    /// Records call events, and whether the slot was still taken when each arrived.
    struct Seen {
        slot: Arc<CallSlot>,
        events: Mutex<Vec<String>>,
    }

    impl Seen {
        fn new(slot: &Arc<CallSlot>) -> Arc<Self> {
            Arc::new(Self { slot: slot.clone(), events: Mutex::default() })
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }

    impl EventSink for Seen {
        fn emit(&self, event: Event) {
            let held = self.slot.holder().is_some();
            let line = match event {
                Event::CallChanged { call: Some(c) } => format!("{:?}", c.state),
                Event::CallChanged { call: None } => format!("none held={held}"),
                Event::CallMissed { token, .. } => format!("missed {token}"),
                _ => return,
            };
            self.events.lock().unwrap().push(line);
        }
    }

    fn client(server: &str) -> TalkClient {
        let account = Account { server: server.into(), user: "u".into(), app_password: "p".into() };
        TalkClient::new(account, Platform::Cli).unwrap()
    }

    fn room(token: &str) -> RoomInfo {
        RoomInfo { token: token.into(), name: "Raum".into(), is_group: false, has_call: true }
    }

    fn ring(m: &CallManager, server: &str, sink: &Arc<Seen>, token: &str, notification: i64) -> bool {
        let rt = tokio::runtime::Handle::current();
        m.ring(client(server), sink.clone(), "me".into(), room(token), "Bob".into(), notification, None, &rt)
    }

    async fn until_free(slot: &CallSlot) {
        for _ in 0..500 {
            if slot.holder().is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the call slot was not freed");
    }

    #[tokio::test]
    async fn one_call_across_accounts() {
        let slot = Arc::new(CallSlot::default());
        let (a, b, c) = (
            CallManager::with_slot(slot.clone()),
            CallManager::with_slot(slot.clone()),
            CallManager::with_slot(slot.clone()),
        );
        let (sa, sb, sc) = (Seen::new(&slot), Seen::new(&slot), Seen::new(&slot));

        // Account A rings: it holds the slot, the others are busy without a call of their own.
        assert!(ring(&a, SERVER, &sa, "t1", 1));
        assert!(a.busy() && a.holds("t1"));
        assert!(!b.busy() && b.busy_anywhere() && !b.holds("t1"));

        // Another call for account B meanwhile: no ring, reported as missed, once.
        assert!(!ring(&b, SERVER, &sb, "t2", 5));
        assert!(!ring(&b, SERVER, &sb, "t2", 5));
        assert_eq!(sb.events(), ["missed t2"]);

        // The same call for account B (both are in the conversation): silent, and
        // remembered, so it does not ring later when A declines.
        assert!(!ring(&b, SERVER, &sb, "t1", 6));
        assert_eq!(sb.events(), ["missed t2"]);
        assert!(b.has_rung("t1", 6));

        // The same token on another server is a different call.
        assert!(!ring(&c, OTHER_SERVER, &sc, "t1", 7));
        assert_eq!(sc.events(), ["missed t1"]);

        // B signing out does not wait for A's call.
        tokio::time::timeout(Duration::from_millis(200), b.hang_up_and_wait(Duration::from_secs(5)))
            .await
            .expect("waited for another account's call");
        assert!(a.busy());

        // A declines: its last event arrives while the slot is still taken, then B can ring.
        a.decline("t1");
        until_free(&slot).await;
        assert!(!a.busy() && a.current().is_none());
        assert_eq!(sa.events().last().map(String::as_str), Some("none held=true"));
        assert!(!ring(&b, SERVER, &sb, "t1", 6), "the call A declined rang for B");
        assert!(ring(&b, SERVER, &sb, "t3", 8));
        assert!(b.busy() && a.busy_anywhere() && !a.busy());
        b.decline("t3");
        until_free(&slot).await;
        assert_eq!(sb.events().last().map(String::as_str), Some("none held=true"));
    }

    #[test]
    fn a_dropped_call_frees_the_slot() {
        let slot = Arc::new(CallSlot::default());
        let sink = Seen::new(&slot);
        let shared = Arc::new(Shared::default());
        let (tx, _rx) = mpsc::unbounded_channel();
        *shared.tx.lock().unwrap() = Some(tx);
        let guard = slot
            .acquire(Holder { owner: 1, server: "s".into(), token: "t".into() })
            .unwrap();
        assert!(slot.acquire(Holder { owner: 2, server: "s".into(), token: "u".into() }).is_none());
        // The call task goes away without its teardown (panic, runtime shut down).
        drop(Live::new(shared.clone(), sink.clone(), guard));
        assert!(slot.holder().is_none());
        assert!(shared.tx.lock().unwrap().is_none());
        assert_eq!(sink.events(), ["none held=true"]);
    }
}
