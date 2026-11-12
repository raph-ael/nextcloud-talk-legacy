//! Message shapes of the standalone signaling server (nextcloud-spreed-signaling, API v1).
//! Only what audio/video calls need (no screen sharing yet). Field names follow the server's Go structs.

use serde_json::{Value, json};

use super::{IceCandidate, SdpType};

/// Room type of all our streams (the clients use "video" for camera+mic streams, also
/// audio only; "screen" is screen sharing).
pub(crate) const ROOM_TYPE: &str = "video";

pub(crate) fn hello_v2(id: &str, backend_url: &str, token: &str) -> Value {
    json!({"id": id, "type": "hello", "hello": {
        "version": "2.0",
        "features": ["chat-relay"],
        "auth": {"url": backend_url, "params": {"token": token}}
    }})
}

pub(crate) fn hello_v1(id: &str, backend_url: &str, user: &str, ticket: &str) -> Value {
    json!({"id": id, "type": "hello", "hello": {
        "version": "1.0",
        "features": ["chat-relay"],
        "auth": {"url": backend_url, "params": {"userid": user, "ticket": ticket}}
    }})
}

pub(crate) fn hello_resume(id: &str, resume_id: &str) -> Value {
    json!({"id": id, "type": "hello", "hello": {"version": "1.0", "resumeid": resume_id}})
}

pub(crate) fn join_room(id: &str, token: &str, nc_session: &str) -> Value {
    json!({"id": id, "type": "room", "room": {"roomid": token, "sessionid": nc_session}})
}

pub(crate) fn leave_room(id: &str) -> Value {
    json!({"id": id, "type": "room", "room": {"roomid": "", "sessionid": ""}})
}

pub(crate) fn bye() -> Value {
    json!({"type": "bye", "bye": {}})
}

fn to_session(session: &str, data: Value) -> Value {
    json!({"type": "message", "message": {
        "recipient": {"type": "session", "sessionid": session},
        "data": data
    }})
}

pub(crate) fn sdp(to: &str, sid: &str, sdp_type: SdpType, sdp: &str, nick: &str) -> Value {
    let t = match sdp_type {
        SdpType::Offer => "offer",
        SdpType::Answer => "answer",
    };
    let mut data = json!({
        "to": to, "type": t, "sid": sid, "roomType": ROOM_TYPE,
        "payload": {"type": t, "sdp": sdp, "nick": nick}
    });
    if sdp_type == SdpType::Offer {
        // Only evaluated for publisher offers (`offer-codecs`): Opus and VP8, which every
        // client (web, official apps, ours) decodes.
        data["audiocodec"] = json!("opus");
        data["videocodec"] = json!("vp8");
    }
    to_session(to, data)
}

pub(crate) fn candidate(to: &str, sid: &str, c: &IceCandidate) -> Value {
    to_session(
        to,
        json!({
            "to": to, "type": "candidate", "sid": sid, "roomType": ROOM_TYPE,
            "payload": {"candidate": {
                "candidate": c.candidate, "sdpMid": c.sdp_mid, "sdpMLineIndex": c.sdp_m_line_index
            }}
        }),
    )
}

/// Ask the MCU to offer the stream of `to`. With `sid` an existing subscription is
/// renegotiated (`update-sdp`), without one a new subscription is created.
pub(crate) fn request_offer(to: &str, sid: Option<&str>) -> Value {
    let mut data = json!({"type": "requestoffer", "roomType": ROOM_TYPE});
    if let Some(sid) = sid {
        data["sid"] = json!(sid);
    }
    to_session(to, data)
}

pub(crate) fn mute(to: &str, muted: bool) -> Value {
    mute_track(to, "audio", muted)
}

/// Camera off (`muted`) or on, as the web client sends it.
pub(crate) fn video(to: &str, on: bool) -> Value {
    mute_track(to, "video", !on)
}

fn mute_track(to: &str, name: &str, muted: bool) -> Value {
    to_session(
        to,
        json!({
            "type": if muted { "mute" } else { "unmute" },
            "roomType": ROOM_TYPE,
            "payload": {"name": name}
        }),
    )
}

/// Simulcast layer of a subscription (0 smallest … 2 full size); the HPB forwards it to
/// the MCU. Publishers without simulcast ignore it.
pub(crate) fn select_stream(to: &str, sid: &str, substream: u8) -> Value {
    to_session(
        to,
        json!({
            "to": to, "sid": sid, "roomType": ROOM_TYPE, "type": "selectStream",
            "payload": {"substream": substream, "temporal": 2}
        }),
    )
}

/// We hang up (sent right before leaving the call). Leaving the call alone looks the
/// same as a rejoin to the others; this lets them end at once. Other clients ignore it.
pub(crate) fn hangup(to: &str) -> Value {
    to_session(to, json!({"type": "hangup", "roomType": ROOM_TYPE}))
}

/// Signaling URL from the settings `server` value (string or list).
pub(crate) fn signaling_url(server: &Value) -> Option<String> {
    let base = match server {
        Value::String(s) => s.clone(),
        Value::Array(list) => list.first()?.as_str()?.to_string(),
        _ => return None,
    };
    if base.is_empty() {
        return None;
    }
    let base = base.trim_end_matches('/');
    let ws = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    };
    Some(if ws.ends_with("/spreed") { ws } else { format!("{ws}/spreed") })
}

pub(crate) fn candidate_from(payload: &Value) -> Option<IceCandidate> {
    let c = payload.get("candidate")?;
    // Some clients nest the candidate object, some send its fields directly.
    let c = if c.is_object() { c } else { payload };
    Some(IceCandidate {
        candidate: c.get("candidate")?.as_str()?.to_string(),
        sdp_mid: c.get("sdpMid").and_then(Value::as_str).map(str::to_string),
        sdp_m_line_index: c.get("sdpMLineIndex").and_then(Value::as_i64).unwrap_or(0) as i32,
    })
}

/// One entry of a `participants` → `update` event.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CallUser {
    pub session_id: String,
    pub in_call: u32,
    pub actor_id: String,
    pub actor_type: String,
    pub internal: bool,
}

pub(crate) fn call_users(list: &Value) -> Vec<CallUser> {
    list.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|u| {
                    Some(CallUser {
                        session_id: u.get("sessionId")?.as_str()?.to_string(),
                        in_call: u.get("inCall").and_then(Value::as_u64).unwrap_or(0) as u32,
                        actor_id: u
                            .get("actorId")
                            .or_else(|| u.get("userId"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        actor_type: u.get("actorType").and_then(Value::as_str).unwrap_or("users").to_string(),
                        internal: u.get("internal").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signaling_url_forms() {
        assert_eq!(
            signaling_url(&json!("https://hpb.example/standalone-signaling/")).as_deref(),
            Some("wss://hpb.example/standalone-signaling/spreed")
        );
        assert_eq!(
            signaling_url(&json!(["https://a.example", "https://b.example"])).as_deref(),
            Some("wss://a.example/spreed")
        );
        assert_eq!(signaling_url(&json!("")), None);
    }

    #[test]
    fn parses_update_users_object_form() {
        let ev = json!({"roomid":"t","users":[
            {"inCall":3,"sessionId":"s1","nextcloudSessionId":"n1","actorType":"users","actorId":"bob","userId":"bob","lastPing":1},
            {"inCall":1,"sessionId":"s2","lastPing":1,"internal":true}
        ]});
        let users = call_users(&ev["users"]);
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].actor_id, "bob");
        assert_eq!(users[0].in_call, 3);
        assert!(users[1].internal);
    }

    #[test]
    fn video_and_select_stream() {
        let off = video("s1", false);
        assert_eq!(off["message"]["data"]["type"], "mute");
        assert_eq!(off["message"]["data"]["payload"]["name"], "video");
        assert_eq!(video("s1", true)["message"]["data"]["type"], "unmute");
        let sel = select_stream("s1", "123", 2);
        assert_eq!(sel["message"]["recipient"]["sessionid"], "s1");
        assert_eq!(sel["message"]["data"]["type"], "selectStream");
        assert_eq!(sel["message"]["data"]["payload"]["substream"], 2);
    }

    #[test]
    fn candidate_nested_and_flat() {
        let nested = json!({"candidate": {"candidate": "candidate:1", "sdpMid": "0", "sdpMLineIndex": 0}});
        assert_eq!(candidate_from(&nested).unwrap().candidate, "candidate:1");
        let flat = json!({"candidate": "candidate:2", "sdpMid": "0", "sdpMLineIndex": 1});
        let c = candidate_from(&flat).unwrap();
        assert_eq!((c.candidate.as_str(), c.sdp_m_line_index), ("candidate:2", 1));
    }
}
