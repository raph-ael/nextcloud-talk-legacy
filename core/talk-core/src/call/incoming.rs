//! Detecting incoming calls while the app runs: notify_push (instant), the room list
//! (`hasCall` turning on) as a safety net, and later decrypted pushes. Every source ends
//! in [`Engine::check_incoming_calls`], which asks the server what actually rings (and
//! picks up recording notices from the same notifications).

use std::sync::{Arc, Weak};
use std::time::Duration;

use serde_json::Value;

use super::ws::{self, Incoming};
use super::RoomInfo;
use crate::client::RingState;
use crate::engine::Engine;
use crate::models::RoomType;

const NOTIFY_BACKOFF_MAX_S: u64 = 60;
/// Without any frame for this long the notify_push socket is considered dead.
const NOTIFY_IDLE: Duration = Duration::from_secs(120);

/// A call that was missed while the app was not connected is still reported if it is
/// not older than this.
const MISSED_REPORT_MAX_AGE_S: i64 = 24 * 60 * 60;

/// Incoming call found in the user's notifications.
struct CallNotification {
    token: String,
    caller: String,
    id: i64,
    /// Unix time the notification was created (0 if unknown).
    at: i64,
}

/// `2026-11-15T20:59:15+00:00` → unix seconds (the notifications app always writes UTC
/// or an offset; 0 for anything else).
fn unix_time(iso: &str) -> i64 {
    let num = |range: std::ops::Range<usize>| iso.get(range).and_then(|s| s.parse::<i64>().ok());
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(sec)) =
        (num(0..4), num(5..7), num(8..10), num(11..13), num(14..16), num(17..19))
    else {
        return 0;
    };
    // Days since 1970-01-01 (civil calendar).
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    let offset = match iso.get(19..) {
        Some(tz) if tz.len() >= 6 && (tz.starts_with('+') || tz.starts_with('-')) => {
            let sign = if tz.starts_with('-') { -1 } else { 1 };
            let hours = tz.get(1..3).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
            let minutes = tz.get(4..6).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
            sign * (hours * 3600 + minutes * 60)
        }
        _ => 0,
    };
    days * 86_400 + h * 3600 + mi * 60 + sec - offset
}

fn call_notifications(list: &[Value]) -> Vec<CallNotification> {
    list.iter()
        .filter(|n| n["app"] == "spreed" && n["object_type"] == "call")
        .filter_map(|n| {
            let token = n["object_id"].as_str()?.to_string();
            let caller = n["subjectRichParameters"]["user"]["name"]
                .as_str()
                .or_else(|| n["subjectRichParameters"]["call"]["name"].as_str())
                .unwrap_or_default()
                .to_string();
            let id = n["notification_id"].as_i64().unwrap_or(0);
            let at = n["datetime"].as_str().map_or(0, unix_time);
            Some(CallNotification { token, caller, id, at })
        })
        .collect()
}

impl Engine {
    /// Looks for calls that ring for us right now and starts ringing (at most one), and
    /// reports one-to-one calls that were missed while the app was not connected, once.
    /// Safe to call often: calls that were dealt with are skipped.
    /// Also updates the recording notices from the same response.
    pub async fn check_incoming_calls(&self) {
        let _one_at_a_time = self.incoming_lock.lock().await;
        let list = match self.client.notifications().await {
            Ok(l) => l,
            Err(e) => {
                tracing::debug!("notifications: {e}");
                return;
            }
        };
        self.update_recording_notices(&list);
        // A call somewhere in this process (any account): nothing else rings, but a call
        // that comes in meanwhile is reported as missed, once (`ring_token` decides).
        let busy = self.calls.busy_anywhere();
        // Call notifications up to this id were dealt with in an earlier check or run.
        // On the very first run there is no mark: then nothing old is reported as missed.
        let mark = *self.call_mark.lock().unwrap();
        let mut seen = mark.unwrap_or(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let mut rang = false;
        let mut calls = call_notifications(&list);
        // Oldest first, so the mark only moves over what was handled.
        calls.sort_by_key(|n| n.id);
        for n in calls {
            if n.id != 0 && n.id <= mark.unwrap_or(0) {
                continue;
            }
            if self.calls.holds(&n.token) || self.calls.has_rung(&n.token, n.id) {
                seen = seen.max(n.id);
                continue;
            }
            let state = match self.client.ring_state(&n.token).await {
                Ok(s) => s,
                // Not known now: look again at the next check.
                Err(_) => break,
            };
            seen = seen.max(n.id);
            // "Do not disturb": like the server, which sends no push then.
            if self.hushed(&n.token) {
                continue;
            }
            match state {
                // Only calls that still ring (the notification stays after a missed call).
                RingState::Ringing if !rang => {
                    rang = self.ring_token(&n.token, &n.caller, n.id, None, true).await && !busy;
                }
                RingState::Ringing => {}
                // Missed while this app was not connected: tell the user once.
                RingState::Missed if mark.is_some() && (n.at == 0 || now - n.at <= MISSED_REPORT_MAX_AGE_S) => {
                    self.report_missed(&n);
                }
                _ => {}
            }
        }
        if Some(seen) != mark {
            *self.call_mark.lock().unwrap() = Some(seen);
            self.cache.set_meta("call_mark", seen.to_string());
        }
    }

    /// A one-to-one call nobody answered while this app was not connected.
    fn report_missed(&self, n: &CallNotification) {
        let Some(room) = self.state_room(&n.token) else { return };
        if !matches!(room.room_type, RoomType::OneToOne | RoomType::FormerOneToOne) {
            return;
        }
        tracing::info!(token = %n.token, "call missed while not connected");
        self.sink.emit(crate::engine::Event::CallMissed {
            token: n.token.clone(),
            room_name: room.display_name.clone(),
            caller_name: if n.caller.is_empty() { room.display_name } else { n.caller.clone() },
            is_group: false,
        });
    }

    /// Rings for a call in `token` (from a push or a notification). `call_id`: id the
    /// platform already used (iOS: the CallKit UUID reported from PushKit). `checked`:
    /// the ring state was verified already; otherwise the ringing call checks it at once
    /// and ends (`missed`) if the call is over, so a reported CallKit call always ends.
    pub(crate) async fn ring_token(
        &self,
        token: &str,
        caller: &str,
        notification_id: i64,
        call_id: Option<String>,
        checked: bool,
    ) -> bool {
        let room = match self.client.room(token).await {
            Ok(r) => Some(r),
            Err(_) => self.state_room(token),
        };
        let info = match &room {
            Some(r) => {
                if checked && !r.has_call {
                    return false;
                }
                RoomInfo {
                    token: r.token.clone(),
                    name: r.display_name.clone(),
                    is_group: !matches!(r.room_type, RoomType::OneToOne | RoomType::FormerOneToOne),
                    has_call: true,
                }
            }
            None => RoomInfo {
                token: token.to_string(),
                name: caller.to_string(),
                is_group: false,
                has_call: true,
            },
        };
        let caller = if caller.is_empty() { info.name.clone() } else { caller.to_string() };
        let Some(rt) = self.runtime.get().cloned() else { return false };
        let rang = self
            .calls
            .ring(self.client.clone(), self.sink.clone(), self.user_id(), info, caller, notification_id, call_id, &rt);
        if rang {
            tracing::info!(token, "incoming call");
        }
        rang
    }

    pub(crate) fn spawn_incoming_check(&self) {
        let Some(rt) = self.runtime.get() else { return };
        let weak = self.weak_self();
        rt.spawn(async move {
            if let Some(e) = weak.upgrade() {
                e.check_incoming_calls().await;
            }
        });
    }

    /// notify_push: a websocket that says "you have a new notification". Runs for the
    /// lifetime of the session; reconnects with backoff.
    pub(crate) async fn notify_push_loop(weak: Weak<Engine>) {
        let mut attempt: u32 = 0;
        loop {
            let Some(engine) = weak.upgrade() else { return };
            let url = match engine.client.capabilities().await {
                Ok(caps) => caps["capabilities"]["notify_push"]["endpoints"]["websocket"]
                    .as_str()
                    .map(str::to_string),
                Err(_) => None,
            };
            let Some(url) = url else {
                // No notify_push on this server (or offline): the room list is the fallback.
                drop(engine);
                attempt = (attempt + 1).min(6);
                tokio::time::sleep(Duration::from_secs(NOTIFY_BACKOFF_MAX_S.min(5 << attempt))).await;
                continue;
            };
            let ok = notify_session(&engine, &url).await;
            drop(engine);
            attempt = if ok { 0 } else { (attempt + 1).min(6) };
            let secs = (1u64 << attempt).min(NOTIFY_BACKOFF_MAX_S);
            tokio::time::sleep(Duration::from_secs(secs)).await;
        }
    }
}

/// One notify_push connection; returns whether it was authenticated (for backoff).
async fn notify_session(engine: &Arc<Engine>, url: &str) -> bool {
    let mut socket = match engine.client.websocket(url).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("notify_push connect: {e}");
            return false;
        }
    };
    let account = engine.client.account().clone();
    if ws::send_text(&mut socket, account.user.clone()).await.is_err()
        || ws::send_text(&mut socket, account.app_password.clone()).await.is_err()
    {
        return false;
    }
    let mut authenticated = false;
    let weak = Arc::downgrade(engine);
    loop {
        let next = tokio::time::timeout(NOTIFY_IDLE, ws::next(&mut socket)).await;
        let Some(engine) = weak.upgrade() else { return authenticated };
        match next {
            Ok(Incoming::Text(t)) => {
                let t = t.trim();
                if t == "authenticated" {
                    tracing::info!("notify_push connected");
                    authenticated = true;
                    // Something may have rung while we were offline.
                    engine.spawn_incoming_check();
                } else if t.starts_with("err:") {
                    tracing::warn!("notify_push: {t}");
                    return authenticated;
                } else if t.starts_with("notify_notification") {
                    engine.spawn_incoming_check();
                    // A chat message makes a notification too: fetch the rooms now, so
                    // the message is announced at once and not at the next poll.
                    engine.spawn_rooms_refresh();
                }
            }
            Ok(Incoming::Alive) => {}
            Ok(Incoming::Closed(r)) => {
                tracing::debug!("notify_push closed: {r}");
                return authenticated;
            }
            Err(_) => {
                // Silent too long: probe with a ping; a dead socket fails on the next read.
                if ws::send_ping(&mut socket).await.is_err() {
                    return authenticated;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_notification_times() {
        assert_eq!(unix_time("1970-01-01T00:00:00+00:00"), 0);
        assert_eq!(unix_time("2026-11-15T20:59:15+00:00"), 1_794_776_355);
        assert_eq!(unix_time("2026-11-15T21:59:15+01:00"), 1_794_776_355);
        assert_eq!(unix_time("2024-02-29T12:00:00Z"), 1_709_208_000);
        assert_eq!(unix_time("gestern"), 0);
    }

    #[test]
    fn finds_call_notifications() {
        let list = vec![
            json!({"app":"spreed","object_type":"chat","object_id":"x"}),
            json!({"app":"spreed","object_type":"call","object_id":"tok","notification_id":7,
                   "subjectRichParameters":{"user":{"name":"Bob"},"call":{"name":"Bob"}}}),
        ];
        let calls = call_notifications(&list);
        assert_eq!(calls.len(), 1);
        assert_eq!((calls[0].token.as_str(), calls[0].caller.as_str(), calls[0].id, calls[0].at), ("tok", "Bob", 7, 0));
    }
}
