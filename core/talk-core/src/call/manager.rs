//! Owner of the single active call: starts the call task and forwards commands to it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use super::actor::{Actor, Cmd, Ctx, Shared};
use super::{CallDirection, CallState, CallView, MediaEngine, MediaReport};
use crate::client::TalkClient;
use crate::engine::{Event, EventSink};
use crate::error::{Error, Result};

/// The same incoming call is not rung again within this time (it arrives from push,
/// notify_push and the room list).
const RING_DEDUP: Duration = Duration::from_secs(90);

/// What the engine knows about a conversation when a call starts.
pub(crate) struct RoomInfo {
    pub token: String,
    pub name: String,
    pub is_group: bool,
    pub has_call: bool,
}

#[derive(Default)]
pub(crate) struct CallManager {
    media: Arc<RwLock<Option<Arc<dyn MediaEngine>>>>,
    shared: Arc<Shared>,
    /// token → when it last rang (or was declined) here.
    rung: Mutex<HashMap<String, Instant>>,
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

    pub(crate) fn busy(&self) -> bool {
        self.shared.tx.lock().unwrap().is_some()
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
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut slot = self.shared.tx.lock().unwrap();
            if slot.is_some() {
                return Err(call_error("busy", "Es läuft bereits ein Anruf"));
            }
            *slot = Some(tx.clone());
        }
        *self.shared.view.lock().unwrap() = Some(view.clone());
        let ctx = Ctx {
            client,
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
        };
        self.launch(client, sink, me, view, silent, runtime)
    }

    /// An incoming call was detected. Returns `false` when it does not ring (another
    /// call in progress, or it rang/was declined here a moment ago).
    pub(crate) fn ring(
        &self,
        client: TalkClient,
        sink: Arc<dyn EventSink>,
        me: String,
        room: RoomInfo,
        caller_name: String,
        runtime: &tokio::runtime::Handle,
    ) -> bool {
        {
            let mut rung = self.rung.lock().unwrap();
            rung.retain(|_, t| t.elapsed() < RING_DEDUP);
            if rung.contains_key(&room.token) {
                return false;
            }
            rung.insert(room.token.clone(), Instant::now());
            if self.busy() {
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
            id: uuid::Uuid::new_v4().to_string(),
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

    pub(crate) fn report(&self, report: MediaReport) {
        self.send(Cmd::Media(report));
    }

    pub(crate) fn network_changed(&self) {
        self.send(Cmd::NetworkChanged);
    }
}
