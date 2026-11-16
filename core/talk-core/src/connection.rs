//! What the server connection looks like to the user (docs/connection.md).
//!
//! One failed request is no outage: the state goes to `connecting`, the core retries at
//! once, and only when the retries keep failing it says `offline`.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::engine::{Engine, Event};
use crate::error::Error;

/// Retries after a failed request. The first one comes at once: a connection that died
/// while the app slept is replaced before the platform shows anything (500 ms).
const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_millis(100),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];
/// ... and then at this pace, until the server answers.
const RETRY_MAX: Duration = Duration::from_secs(5);
/// A retry that hangs is given up after this long.
const RETRY_TIMEOUT: Duration = Duration::from_secs(10);
/// A retry failing this long after the first failure makes an outage (3.6 s when requests
/// fail at once, long enough for a change of network) ...
const OFFLINE_AFTER_FAILING: Duration = Duration::from_secs(3);
/// ... or this long without an answer (requests that hang instead of failing).
const OFFLINE_AFTER: Duration = Duration::from_secs(8);
/// The same when the platform said there is no network: nothing can answer.
const OFFLINE_AFTER_LOST: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionState {
    /// The last server call was answered.
    Online,
    /// No answer yet after the start, or a call just failed and the core is retrying.
    Connecting,
    /// The retries failed too. The core keeps trying; only an answer ends this state.
    Offline,
}

pub(crate) struct Conn {
    state: ConnectionState,
    /// A call failed and none was answered since.
    failing: bool,
    /// Counts outages, so the timer of an earlier one changes nothing.
    outage: u64,
    /// When the first call of this outage failed.
    since: Option<Instant>,
    /// The retry task is running.
    retrying: bool,
    /// The session was shut down: no more retries.
    stopped: bool,
}

impl Default for Conn {
    fn default() -> Self {
        Self { state: ConnectionState::Connecting, failing: false, outage: 0, since: None, retrying: false, stopped: false }
    }
}

impl Engine {
    pub fn connection_state(&self) -> ConnectionState {
        self.conn.lock().unwrap().state
    }

    pub(crate) fn is_offline(&self) -> bool {
        self.connection_state() == ConnectionState::Offline
    }

    /// A server call was answered. Without a chat list that is not `online` yet: an empty
    /// list means "still loading" to the platform until the list is there, so the state
    /// waits for it (the retries and the regular refresh fetch it).
    pub(crate) fn reached(&self) {
        if !self.list_known() {
            return;
        }
        {
            let mut c = self.conn.lock().unwrap();
            c.failing = false;
            c.since = None;
            if c.state == ConnectionState::Online {
                return;
            }
            c.state = ConnectionState::Online;
        }
        self.announce();
    }

    /// A server call got no answer: `connecting`, and retry soon instead of waiting for
    /// the next regular refresh.
    pub(crate) fn unreachable(&self) {
        self.failing(OFFLINE_AFTER);
    }

    /// The platform has no network at all: `connecting` now instead of after a request
    /// that hangs until its timeout (a socket of a network that went away does not always
    /// fail). Calls and the outbox notice it themselves.
    pub fn network_lost(&self) {
        self.failing(OFFLINE_AFTER_LOST);
    }

    fn failing(&self, deadline: Duration) {
        let outage = {
            let mut c = self.conn.lock().unwrap();
            if c.failing {
                return;
            }
            c.failing = true;
            c.since = Some(Instant::now());
            c.outage += 1;
            if c.state == ConnectionState::Online {
                c.state = ConnectionState::Connecting;
            }
            c.outage
        };
        self.announce();
        let Some(rt) = self.handle() else {
            // Nothing to retry with: as before, a failed call is an outage.
            self.set_offline(outage);
            return;
        };
        let weak = self.weak_self();
        rt.spawn(async move {
            tokio::time::sleep(deadline).await;
            if let Some(engine) = weak.upgrade() {
                engine.set_offline(outage);
            }
        });
        self.spawn_retries(false);
    }

    /// The platform saw the network come back or change, or the app came to the front:
    /// retry now. While `offline` the state stays as it is until a call is answered, so it
    /// does not flicker. Without a failure the list is fetched all the same: it is what the
    /// user looks at next, and a connection that died meanwhile shows now, not in ten seconds.
    pub(crate) fn retry_now(&self) {
        if self.conn.lock().unwrap().failing {
            self.spawn_retries(true);
        } else {
            self.spawn_rooms_refresh();
        }
    }

    /// The session ends: the retries stop with it.
    pub(crate) fn stop_retries(&self) {
        self.conn.lock().unwrap().stopped = true;
        self.retry_wake.notify_one();
    }

    fn handle(&self) -> Option<tokio::runtime::Handle> {
        self.runtime.get().cloned().or_else(|| tokio::runtime::Handle::try_current().ok())
    }

    fn set_offline(&self, outage: u64) {
        {
            let mut c = self.conn.lock().unwrap();
            if !c.failing || c.outage != outage || c.state != ConnectionState::Connecting {
                return;
            }
            c.state = ConnectionState::Offline;
        }
        self.announce();
    }

    /// A session starts: `offline` if nothing was answered in time (a server that accepts
    /// the connection and then hangs fails only after the request timeout).
    pub(crate) fn await_first_answer(&self) {
        self.conn.lock().unwrap().stopped = false;
        let Some(rt) = self.handle() else { return };
        let weak = self.weak_self();
        rt.spawn(async move {
            tokio::time::sleep(OFFLINE_AFTER).await;
            let Some(engine) = weak.upgrade() else { return };
            {
                let mut c = engine.conn.lock().unwrap();
                // Still the first state: nothing answered, nothing failed.
                if c.state != ConnectionState::Connecting || c.failing || c.outage != 0 {
                    return;
                }
                c.state = ConnectionState::Offline;
            }
            engine.announce();
        });
    }

    /// Fetches the room list after 0.1 s, 0.5 s, 1 s, 2 s, 4 s and then every 5 s until the
    /// server answers.
    fn spawn_retries(&self, now: bool) {
        let Some(rt) = self.handle() else { return };
        {
            let mut c = self.conn.lock().unwrap();
            if c.stopped {
                return;
            }
            if c.retrying {
                drop(c);
                if now {
                    self.retry_wake.notify_one();
                }
                return;
            }
            c.retrying = true;
        }
        let weak = self.weak_self();
        let wake = self.retry_wake.clone();
        rt.spawn(async move {
            let mut step = 0;
            let mut at_once = now;
            loop {
                let delay = if at_once { Duration::ZERO } else { RETRY_DELAYS.get(step).copied().unwrap_or(RETRY_MAX) };
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = wake.notified() => {}
                }
                let Some(engine) = weak.upgrade() else { return };
                let outage = {
                    let c = engine.conn.lock().unwrap();
                    (c.failing && !c.stopped).then_some(c.outage)
                };
                // Another call got through meanwhile (or the session ended).
                let Some(outage) = outage else { break };
                let answered = tokio::select! {
                    r = tokio::time::timeout(RETRY_TIMEOUT, engine.refresh_rooms(false)) => {
                        matches!(r, Ok(Ok(()) | Err(Error::Unauthorized)))
                    }
                    // The network is back: do not wait for a request that hangs on the old one.
                    _ = wake.notified() => {
                        at_once = true;
                        continue;
                    }
                };
                if answered {
                    break;
                }
                engine.retry_failed(outage);
                step = if at_once { 1 } else { step + 1 };
                at_once = false;
            }
            if let Some(engine) = weak.upgrade() {
                engine.conn.lock().unwrap().retrying = false;
            }
        });
    }

    fn retry_failed(&self, outage: u64) {
        let offline = {
            let c = self.conn.lock().unwrap();
            if !c.failing || c.outage != outage {
                return;
            }
            c.since.is_some_and(|t| t.elapsed() >= OFFLINE_AFTER_FAILING)
        };
        if offline {
            self.set_offline(outage);
        }
    }

    /// Tells the platform the state if it differs from what it was told last. One at a
    /// time, so two changes in a row arrive in their order.
    fn announce(&self) {
        let mut told = self.conn_told.lock().unwrap();
        let state = self.connection_state();
        let before = std::mem::replace(&mut *told, state);
        if before == state {
            return;
        }
        self.sink.emit(Event::Connection { state });
        if state == ConnectionState::Online {
            self.outbox.wake();
        }
        // "Wartet auf Netz" depends on it (queued messages).
        if before == ConnectionState::Offline || state == ConnectionState::Offline {
            self.outbox_views_changed();
        }
    }
}
