//! What the server connection looks like to the user (docs/connection.md).
//!
//! One failed request is no outage: the state goes to `connecting`, the core retries at
//! once, and only when the retries fail as well it says `offline`.

use std::time::Duration;

use serde::Serialize;

use crate::engine::{Engine, Event};
use crate::error::Error;

/// First retry after a failed request, then twice as long each time.
const RETRY_FIRST: Duration = Duration::from_millis(500);
/// Quick retries per outage; after that the regular room list refresh keeps trying.
const RETRIES: u32 = 4;
/// So many failed retries make an outage ...
const OFFLINE_AFTER_RETRIES: u32 = 2;
/// ... or this long without an answer (requests that hang instead of failing).
const OFFLINE_AFTER: Duration = Duration::from_secs(8);

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
    failed_retries: u32,
    /// The retry task is running.
    retrying: bool,
}

impl Default for Conn {
    fn default() -> Self {
        Self { state: ConnectionState::Connecting, failing: false, outage: 0, failed_retries: 0, retrying: false }
    }
}

impl Engine {
    pub fn connection_state(&self) -> ConnectionState {
        self.conn.lock().unwrap().state
    }

    pub(crate) fn is_offline(&self) -> bool {
        self.connection_state() == ConnectionState::Offline
    }

    /// A server call was answered.
    pub(crate) fn reached(&self) {
        {
            let mut c = self.conn.lock().unwrap();
            c.failing = false;
            c.failed_retries = 0;
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
        let outage = {
            let mut c = self.conn.lock().unwrap();
            if c.failing {
                return;
            }
            c.failing = true;
            c.failed_retries = 0;
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
            tokio::time::sleep(OFFLINE_AFTER).await;
            if let Some(engine) = weak.upgrade() {
                engine.set_offline(outage);
            }
        });
        self.spawn_retries(false);
    }

    /// The platform saw the network come back or change: retry now. While `offline` the
    /// state stays as it is until a call is answered, so it does not flicker.
    pub(crate) fn retry_now(&self) {
        if self.conn.lock().unwrap().failing {
            self.spawn_retries(true);
        }
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

    /// Fetches the room list after 0.5 s, 1 s, 2 s and 4 s until the server answers.
    fn spawn_retries(&self, now: bool) {
        let Some(rt) = self.handle() else { return };
        {
            let mut c = self.conn.lock().unwrap();
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
            let mut delay = if now { Duration::ZERO } else { RETRY_FIRST };
            let mut left = RETRIES;
            while left > 0 {
                let woken = tokio::select! {
                    _ = tokio::time::sleep(delay) => false,
                    _ = wake.notified() => true,
                };
                let Some(engine) = weak.upgrade() else { return };
                let outage = {
                    let c = engine.conn.lock().unwrap();
                    c.failing.then_some(c.outage)
                };
                // Another call got through meanwhile.
                let Some(outage) = outage else { break };
                if woken {
                    left = RETRIES;
                    delay = Duration::ZERO;
                }
                match engine.refresh_rooms(false).await {
                    Ok(()) | Err(Error::Unauthorized) => break,
                    Err(_) => engine.retry_failed(outage),
                }
                left -= 1;
                delay = if delay.is_zero() { RETRY_FIRST } else { delay * 2 };
            }
            if let Some(engine) = weak.upgrade() {
                engine.conn.lock().unwrap().retrying = false;
            }
        });
    }

    fn retry_failed(&self, outage: u64) {
        let offline = {
            let mut c = self.conn.lock().unwrap();
            if !c.failing || c.outage != outage {
                return;
            }
            c.failed_retries += 1;
            c.failed_retries >= OFFLINE_AFTER_RETRIES
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
