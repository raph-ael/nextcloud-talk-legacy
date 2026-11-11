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

/// Incoming call found in the user's notifications.
struct CallNotification {
    token: String,
    caller: String,
    id: i64,
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
            Some(CallNotification { token, caller, id })
        })
        .collect()
}

impl Engine {
    /// Looks for calls that ring for us right now and starts ringing (at most one).
    /// Safe to call often: calls that rang already are ignored.
    /// Also updates the recording notices from the same response.
    pub async fn check_incoming_calls(&self) {
        let list = match self.client.notifications().await {
            Ok(l) => l,
            Err(e) => {
                tracing::debug!("notifications: {e}");
                return;
            }
        };
        self.update_recording_notices(&list);
        if self.calls.busy() {
            return;
        }
        for n in call_notifications(&list) {
            // Only calls that still ring (the notification stays after a missed call).
            match self.client.ring_state(&n.token).await {
                Ok(RingState::Ringing) => {}
                _ => continue,
            }
            if self.ring_token(&n.token, &n.caller, n.id, None, true).await {
                return;
            }
        }
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
    fn finds_call_notifications() {
        let list = vec![
            json!({"app":"spreed","object_type":"chat","object_id":"x"}),
            json!({"app":"spreed","object_type":"call","object_id":"tok","notification_id":7,
                   "subjectRichParameters":{"user":{"name":"Bob"},"call":{"name":"Bob"}}}),
        ];
        let calls = call_notifications(&list);
        assert_eq!(calls.len(), 1);
        assert_eq!((calls[0].token.as_str(), calls[0].caller.as_str(), calls[0].id), ("tok", "Bob", 7));
    }
}
