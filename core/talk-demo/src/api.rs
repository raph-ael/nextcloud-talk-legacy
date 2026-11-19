//! OCS endpoints of Nextcloud and Talk (spreed) as the clients use them
//! (core/talk-core/src/client.rs).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Path, State as AxState};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::http::{Params, bad_request, not_found, ocs, ocs_status, ocs_with_headers};
use crate::state::{Member, Reaction, Room, State, Tag, now};

/// Rendered previews by (file id, width, height).
pub type PreviewCache = std::collections::HashMap<(i64, u32, u32), Arc<Vec<u8>>>;

pub struct AppState {
    pub st: Mutex<State>,
    pub previews: Mutex<PreviewCache>,
}

pub type App = Arc<AppState>;
type S = AxState<App>;

// --- server, user, login -----------------------------------------------------------------

pub async fn status_php() -> Response {
    axum::Json(json!({
        "installed": true,
        "maintenance": false,
        "needsDbUpgrade": false,
        "version": "31.0.0.0",
        "versionstring": "31.0.0",
        "edition": "",
        "productname": "Demo-Cloud",
        "extendedSupport": false,
    }))
    .into_response()
}

pub async fn root() -> Response {
    (
        [("content-type", "text/plain; charset=utf-8")],
        "Talk-Demo-Server: erfundene Inhalte für Screenshots. Beliebiger Benutzer und beliebiges Passwort.\n",
    )
        .into_response()
}

pub async fn capabilities(AxState(app): S) -> Response {
    let sort_order = app.st.lock().unwrap().sort_order.clone();
    ocs(json!({
        "version": { "major": 31, "minor": 0, "micro": 0, "string": "31.0.0", "edition": "", "extendedSupport": false },
        "capabilities": {
            "core": { "pollinterval": 60, "webdav-root": "remote.php/webdav", "reference-api": false },
            "theming": { "name": "Demo-Cloud", "url": "", "slogan": "", "color": "#4A90E2", "color-text": "#ffffff" },
            "spreed": {
                "features": [
                    "audio", "video", "chat-v2", "conversation-v4", "reactions", "react-permission",
                    "conversation-tags", "favorites", "archived-conversations-v2", "chat-read-marker",
                    "chat-unread", "chat-read-last", "voice-message-sharing", "markdown-messages",
                    "rich-object-sharing", "silent-send", "read-only-rooms", "notification-levels",
                    "mention-flag", "last-room-activity", "avatar", "conversation-creation-all",
                    "delete-messages", "delete-messages-unlimited", "edit-messages", "edit-messages-note-to-self",
                    "chat-get-context", "unified-search"
                ],
                "config": {
                    "attachments": { "allowed": true, "folder": "/Talk" },
                    "chat": { "max-length": 32000, "read-privacy": 0, "typing-privacy": 1, "has-translation-providers": false },
                    "conversations": { "can-create": true, "force-passwords": false, "sort-order": sort_order },
                    "call": { "enabled": true, "recording": false, "can-upload-background": false },
                    "previews": { "max-gif-size": 3145728 },
                    "signaling": { "session-ping-limit": 200 }
                },
                "version": "21.0.0"
            },
            "notifications": { "ocs-endpoints": ["list", "get", "delete", "delete-all", "rich-strings"], "push": ["devices", "object-data", "delete"] }
        }
    }))
}

pub async fn cloud_user(AxState(app): S) -> Response {
    let st = app.st.lock().unwrap();
    ocs(json!({
        "id": st.me,
        "displayname": st.name_of(&st.me),
        "display-name": st.name_of(&st.me),
        "email": null,
        "language": "de",
        "locale": "de_DE",
        "enabled": true,
    }))
}

pub async fn get_app_password() -> Response {
    ocs(json!({ "apppassword": format!("demo-{}", uuid::Uuid::new_v4().simple()) }))
}

pub async fn delete_app_password() -> Response {
    ocs(json!([]))
}

/// Only call notifications (missed calls), in the shape of the notifications app.
pub async fn notifications(AxState(app): S) -> Response {
    let st = app.st.lock().unwrap();
    let list: Vec<Value> = st
        .call_notices
        .iter()
        .rev()
        .map(|n| {
            let name = st.name_of(&n.caller);
            json!({
                "notification_id": n.id,
                "app": "spreed",
                "user": st.me,
                "datetime": iso_utc(n.ts),
                "object_type": "call",
                "object_id": n.token,
                "subject": format!("Du hast einen Anruf von {name} verpasst"),
                "subjectRichParameters": { "user": { "type": "user", "id": n.caller, "name": name } },
            })
        })
        .collect();
    ocs(json!(list))
}

/// Unix seconds as `2026-11-15T20:59:15+00:00`.
fn iso_utc(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    // Civil date from days since 1970-01-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// The signed-in user's own status.
pub async fn user_status(AxState(app): S) -> Response {
    let st = app.st.lock().unwrap();
    ocs(json!({ "userId": st.me, "status": st.status, "message": null, "icon": null, "clearAt": null }))
}

/// `GET call/{token}/notification-state`: 201 for a missed call, else 404 (nothing rings
/// here: the demo has no calls).
pub async fn call_notification_state(AxState(app): S, Path(token): Path<String>) -> Response {
    let st = app.st.lock().unwrap();
    if st.call_notices.iter().any(|n| n.token == token) {
        ocs_status(StatusCode::CREATED, json!([]))
    } else {
        not_found()
    }
}

/// Test hook: the user's status, e.g. `status=dnd` ("do not disturb") or `status=online`.
pub async fn demo_status(AxState(app): S, p: Params) -> Response {
    let status = p.str("status").unwrap_or_default();
    if !["online", "away", "dnd", "invisible", "offline"].contains(&status.as_str()) {
        return bad_request("status");
    }
    app.st.lock().unwrap().status = status.clone();
    ocs(json!({ "status": status }))
}

/// Test hook: somebody called in a one-to-one conversation and nobody answered. `token`,
/// optionally `from` and `ago` (seconds in the past, default 0).
pub async fn demo_missed_call(AxState(app): S, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let token = p.str("token").unwrap_or_default();
    let Some(room) = st.room(&token) else { return not_found() };
    let from = p
        .str("from")
        .or_else(|| room.members.iter().find(|m| m.person != st.me).map(|m| m.person.clone()))
        .unwrap_or_default();
    if from == st.me || !room.is_member(&from) {
        return bad_request("from");
    }
    let id = st.call_notices.iter().map(|n| n.id).max().unwrap_or(0) + 1;
    let ts = crate::state::now() - p.i64("ago").unwrap_or(0).max(0);
    st.call_notices.push(crate::state::CallNotice { id, token, caller: from, ts });
    ocs_status(StatusCode::CREATED, json!({ "notification_id": id }))
}

pub async fn autocomplete(AxState(app): S, p: Params) -> Response {
    let st = app.st.lock().unwrap();
    let q = p.str("search").unwrap_or_default().to_lowercase();
    let limit = p.i64("limit").unwrap_or(20).max(1) as usize;
    let hits: Vec<Value> = st
        .people
        .iter()
        .filter(|x| x.id != st.me && (x.name.to_lowercase().contains(&q) || x.id.contains(&q)))
        .take(limit)
        .map(|x| json!({ "id": x.id, "label": x.name, "icon": "icon-user", "source": "users", "subline": "", "status": [], "shareWithDisplayNameUnique": x.id }))
        .collect();
    ocs(Value::Array(hits))
}

// --- conversations -------------------------------------------------------------------------

pub async fn rooms(AxState(app): S, p: Params) -> Response {
    let st = app.st.lock().unwrap();
    let since = p.i64("modifiedSince").unwrap_or(0);
    let mut list: Vec<&Room> = st
        .rooms
        .iter()
        .filter(|r| r.is_member(&st.me) && (since == 0 || r.modified >= since))
        .collect();
    list.sort_by_key(|r| std::cmp::Reverse(r.messages.iter().filter(|m| m.visible()).map(|m| m.ts).max().unwrap_or(r.created)));
    let data: Vec<Value> = list.into_iter().map(|r| st.room_json(r)).collect();
    ocs_with_headers(
        StatusCode::OK,
        Value::Array(data),
        &[
            ("x-nextcloud-talk-modified-before", now().to_string()),
            ("x-nextcloud-talk-hash", "demo0000000000000000000000000001".into()),
        ],
    )
}

pub async fn room(AxState(app): S, Path(token): Path<String>) -> Response {
    let st = app.st.lock().unwrap();
    match st.room(&token) {
        Some(r) => ocs(st.room_json(r)),
        None => not_found(),
    }
}

/// `POST /room`: group (`roomName`, `participants[users][]`) or one-to-one (`invite`).
pub async fn create_room(AxState(app): S, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let kind = p.i64("roomType").unwrap_or(2);
    let me = st.me.clone();
    if kind == 1 {
        let Some(other) = p.str("invite").filter(|u| st.person(u).is_some()) else { return bad_request("invite") };
        if let Some(r) = st.rooms.iter().find(|r| r.kind == 1 && r.is_member(&me) && r.is_member(&other)) {
            return ocs(st.room_json(r));
        }
        let room = new_room(&mut st, 1, &other, vec![me.clone(), other.clone()]);
        let json = st.room_json(&room);
        st.rooms.push(room);
        return ocs_status(StatusCode::CREATED, json);
    }
    let name = p.str("roomName").unwrap_or_default();
    if name.trim().is_empty() {
        return bad_request("name");
    }
    let mut members = vec![me.clone()];
    for u in p.list("participants[users]") {
        if st.person(&u).is_some() && !members.contains(&u) {
            members.push(u);
        }
    }
    let mut room = new_room(&mut st, kind.clamp(2, 3) as u8, name.trim(), members);
    let created = st.system_msg("conversation_created", "Du hast die Unterhaltung erstellt".into());
    let token = room.token.clone();
    room.messages.clear();
    st.rooms.push(room);
    st.push_msg(&token, created);
    let r = st.room(&token).unwrap();
    ocs_status(StatusCode::CREATED, st.room_json(r))
}

fn new_room(st: &mut State, kind: u8, name: &str, members: Vec<String>) -> Room {
    let id = st.next_room;
    st.next_room += 1;
    let avatar = if kind == 1 { members.get(1).and_then(|o| st.person(o)).and_then(|p| p.avatar.clone()) } else { None };
    Room {
        id,
        token: format!("demo{id:04}"),
        kind,
        name: name.into(),
        description: String::new(),
        groups: Vec::new(),
        teams: Vec::new(),
        members: members
            .into_iter()
            .enumerate()
            .map(|(i, person)| Member { ptype: if i == 0 || kind == 1 { 1 } else { 3 }, attendee_id: id * 100 + i as i64 + 1, person })
            .collect(),
        favorite: false,
        archived: false,
        notification_level: 0,
        tag_ids: Vec::new(),
        avatar_version: if avatar.is_some() { format!("p{id}") } else { "0".into() },
        avatar,
        custom_avatar: false,
        last_read: 0,
        messages: Vec::new(),
        modified: now(),
        created: now(),
        tx: tokio::sync::watch::channel(0).0,
        nothing_newer: None,
    }
}

pub async fn rename_room(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let name = p.str("roomName").unwrap_or_default().trim().to_string();
    if name.is_empty() {
        return bad_request("name");
    }
    let Some(r) = st.room_mut(&token) else { return not_found() };
    r.name = name.clone();
    let m = st.system_msg("conversation_renamed", format!("Du hast die Unterhaltung in „{name}“ umbenannt"));
    st.push_msg(&token, m);
    ocs(json!([]))
}

pub async fn set_description(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let text = p.str("description").unwrap_or_default();
    let Some(r) = st.room_mut(&token) else { return not_found() };
    r.description = text.clone();
    let m = if text.is_empty() {
        st.system_msg("description_removed", "Du hast die Beschreibung entfernt".into())
    } else {
        st.system_msg("description_set", format!("Du hast die Beschreibung auf „{text}“ gesetzt"))
    };
    st.push_msg(&token, m);
    ocs(json!([]))
}

pub async fn delete_room(AxState(app): S, Path(token): Path<String>) -> Response {
    let mut st = app.st.lock().unwrap();
    let before = st.rooms.len();
    st.rooms.retain(|r| r.token != token);
    if st.rooms.len() == before { not_found() } else { ocs(json!([])) }
}

fn room_flag(app: &App, token: &str, f: impl FnOnce(&mut Room)) -> Response {
    let mut st = app.st.lock().unwrap();
    let Some(r) = st.room_mut(token) else { return not_found() };
    f(r);
    r.touch();
    let r = st.room(token).unwrap();
    ocs(st.room_json(r))
}

pub async fn favorite_on(AxState(app): S, Path(token): Path<String>) -> Response {
    room_flag(&app, &token, |r| r.favorite = true)
}

pub async fn favorite_off(AxState(app): S, Path(token): Path<String>) -> Response {
    room_flag(&app, &token, |r| r.favorite = false)
}

pub async fn archive_on(AxState(app): S, Path(token): Path<String>) -> Response {
    room_flag(&app, &token, |r| r.archived = true)
}

pub async fn archive_off(AxState(app): S, Path(token): Path<String>) -> Response {
    room_flag(&app, &token, |r| r.archived = false)
}

pub async fn notify_level(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let level = p.i64("level").unwrap_or(0).clamp(0, 3) as u8;
    room_flag(&app, &token, |r| r.notification_level = level)
}

/// The user settings Talk keeps in Nextcloud's user config; we know the sort order only.
pub async fn user_config(AxState(app): S, Path(key): Path<String>, p: Params) -> Response {
    match (key.as_str(), p.str("configValue")) {
        ("conversations_sort_order", Some(v)) if v == "activity" || v == "alphabetical" => {
            app.st.lock().unwrap().sort_order = v;
            ocs(json!([]))
        }
        _ => bad_request("invalid"),
    }
}

pub async fn room_tags(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let ids = p.list("tagIds");
    let known: Vec<String> = app.st.lock().unwrap().tags.iter().filter(|t| t.kind == "custom").map(|t| t.id.clone()).collect();
    let ids: Vec<String> = ids.into_iter().filter(|i| known.contains(i)).collect();
    room_flag(&app, &token, |r| r.tag_ids = ids)
}

// --- members and sessions --------------------------------------------------------------------

pub async fn participants(AxState(app): S, Path(token): Path<String>) -> Response {
    let st = app.st.lock().unwrap();
    match st.room(&token) {
        Some(r) => ocs(st.participants_json(r)),
        None => not_found(),
    }
}

pub async fn add_participant(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let Some(user) = p.str("newParticipant").filter(|u| st.person(u).is_some()) else { return not_found() };
    let text = st.added_text(&st.me.clone(), &user);
    let param = st.user_param(&user);
    let Some(r) = st.room_mut(&token) else { return not_found() };
    if r.is_member(&user) {
        return ocs(json!([]));
    }
    let attendee_id = r.members.iter().map(|m| m.attendee_id).max().unwrap_or(r.id * 100) + 1;
    r.members.push(Member { person: user, ptype: 3, attendee_id });
    let mut m = st.system_msg("user_added", text);
    m.params.insert("user".into(), param);
    st.push_msg(&token, m);
    ocs(json!([]))
}

pub async fn remove_attendee(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let id = p.i64("attendeeId").unwrap_or(0);
    let Some(r) = st.room_mut(&token) else { return not_found() };
    let Some(pos) = r.members.iter().position(|m| m.attendee_id == id) else { return not_found() };
    let user = r.members.remove(pos).person;
    let text = format!("Du hast {} entfernt", st.name_of(&user));
    let m = st.system_msg("user_removed", text);
    st.push_msg(&token, m);
    ocs(json!([]))
}

async fn set_moderator(app: App, token: String, p: Params, on: bool) -> Response {
    let mut st = app.st.lock().unwrap();
    let id = p.i64("attendeeId").unwrap_or(0);
    let Some(r) = st.room_mut(&token) else { return not_found() };
    let Some(m) = r.members.iter_mut().find(|m| m.attendee_id == id) else { return not_found() };
    m.ptype = if on { 2 } else { 3 };
    let user = m.person.clone();
    let text = if on {
        format!("Du hast {} zum Moderator gemacht", st.name_of(&user))
    } else {
        format!("Du hast {} die Moderation entzogen", st.name_of(&user))
    };
    let msg = st.system_msg(if on { "moderator_promoted" } else { "moderator_demoted" }, text);
    st.push_msg(&token, msg);
    ocs(json!([]))
}

pub async fn promote(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    set_moderator(app, token, p, true).await
}

pub async fn demote(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    set_moderator(app, token, p, false).await
}

pub async fn leave_conversation(AxState(app): S, Path(token): Path<String>) -> Response {
    let mut st = app.st.lock().unwrap();
    let me = st.me.clone();
    let Some(r) = st.room_mut(&token) else { return not_found() };
    r.members.retain(|m| m.person != me);
    r.touch();
    ocs(json!([]))
}

pub async fn join(AxState(app): S, Path(token): Path<String>) -> Response {
    let st = app.st.lock().unwrap();
    let Some(r) = st.room(&token) else { return not_found() };
    let mut data = st.room_json(r);
    data["sessionId"] = format!("demo-session-{}", uuid::Uuid::new_v4().simple()).into();
    ocs(data)
}

pub async fn leave(AxState(app): S, Path(token): Path<String>) -> Response {
    let st = app.st.lock().unwrap();
    if st.room(&token).is_none() {
        return not_found();
    }
    ocs(json!([]))
}

// --- chat -------------------------------------------------------------------------------

fn chat_headers(last_given: Option<i64>, common: i64) -> Vec<(&'static str, String)> {
    let mut h = vec![("x-chat-last-common-read", common.to_string())];
    if let Some(g) = last_given {
        h.push(("x-chat-last-given", g.to_string()));
    }
    h
}

/// How long Talk remembers that nothing is newer than the id a long poll asked with.
const NOTHING_NEWER_FOR: Duration = Duration::from_secs(30);

/// History (`lookIntoFuture=0`, newest first) or long poll (`lookIntoFuture=1`: waits up
/// to `timeout` seconds for new messages, then 304).
///
/// Like Talk, the long poll remembers for 30 s that nothing is newer than the id it was
/// asked with and answers every request with that id from this memory. A new message
/// clears it, a new reaction does not: a waiting poll sees the reaction when the memory
/// has run out, a request with another id sees it at once.
pub async fn chat_get(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let future = p.i64("lookIntoFuture") == Some(1);
    let limit = p.i64("limit").unwrap_or(100).clamp(1, 200) as usize;
    let last_known = p.i64("lastKnownMessageId").unwrap_or(0);
    let timeout = p.i64("timeout").unwrap_or(30).clamp(0, 60) as u64;
    let set_read = p.bool("setReadMarker").unwrap_or(true);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        let mut rx = {
            let mut st = app.st.lock().unwrap();
            let Some(room) = st.room(&token) else { return not_found() };
            let rx = room.tx.subscribe();
            let remembered = room.nothing_newer.filter(|(id, until)| future && *id == last_known && Instant::now() < *until);
            let picked: Vec<&crate::state::Msg> = if remembered.is_some() {
                Vec::new()
            } else if future {
                room.messages.iter().filter(|m| m.id > last_known).take(limit).collect()
            } else {
                let older: Vec<_> = room.messages.iter().filter(|m| last_known <= 0 || m.id < last_known).collect();
                older.into_iter().rev().take(limit).collect()
            };
            if !picked.is_empty() {
                let data: Vec<Value> = picked.iter().map(|m| st.msg_json(room, m, true)).collect();
                // Future: the newest returned; history: the oldest returned (always the last one).
                let given = picked.last().map(|m| m.id);
                let newest = picked.iter().map(|m| m.id).max().unwrap_or(0);
                let headers = chat_headers(given, room.common_read());
                if future && set_read
                    && let Some(r) = st.room_mut(&token)
                    && newest > r.last_read
                {
                    r.last_read = newest;
                    r.modified = now();
                }
                return ocs_with_headers(StatusCode::OK, Value::Array(data), &headers);
            }
            if !future {
                return StatusCode::NOT_MODIFIED.into_response();
            }
            let until = match remembered {
                Some((_, until)) => until,
                None => {
                    let until = Instant::now() + NOTHING_NEWER_FOR;
                    if let Some(r) = st.room_mut(&token) {
                        r.nothing_newer = Some((last_known, until));
                    }
                    until
                }
            };
            (rx, until)
        };
        let (rx, until) = &mut rx;
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return StatusCode::NOT_MODIFIED.into_response();
        }
        // Wakes for a change of the room, or to look again once the memory has run out.
        let wait = left.min(until.saturating_duration_since(Instant::now()) + Duration::from_millis(10));
        let _ = tokio::time::timeout(wait, rx.changed()).await;
    }
}

/// The messages around one message (`chat-get-context`): `limit` before it, the message,
/// `limit` after it. Leaves the read marker alone.
pub async fn chat_context(AxState(app): S, Path((token, id)): Path<(String, i64)>, p: Params) -> Response {
    let st = app.st.lock().unwrap();
    let limit = p.i64("limit").unwrap_or(50).clamp(0, 100) as usize;
    let Some(room) = st.room(&token) else { return not_found() };
    if room.msg(id).is_none() {
        return not_found();
    }
    let mut picked: Vec<&crate::state::Msg> = room.messages.iter().filter(|m| m.id <= id).rev().take(limit + 1).collect();
    picked.extend(room.messages.iter().filter(|m| m.id > id).take(limit));
    let data: Vec<Value> = picked.iter().map(|m| st.msg_json(room, m, true)).collect();
    ocs_with_headers(StatusCode::OK, Value::Array(data), &chat_headers(picked.last().map(|m| m.id), room.common_read()))
}

/// Unified search, provider `talk-message`: messages that contain `term`, newest first,
/// in all conversations or in the one of the filter `conversation`. Like Talk it looks
/// at the stored message (not at file names), takes the term as one sequence of
/// characters and pages by offset (`cursor`).
pub async fn search_messages(AxState(app): S, p: Params) -> Response {
    let st = app.st.lock().unwrap();
    let term = p.str("term").unwrap_or_default().to_lowercase();
    if term.is_empty() {
        return bad_request("No valid filters provided");
    }
    let limit = p.i64("limit").unwrap_or(5).clamp(1, 25) as usize;
    let offset = p.i64("cursor").unwrap_or(0).max(0) as usize;
    let conversation = p.str("conversation");
    let mut found: Vec<(&Room, &crate::state::Msg, String)> = Vec::new();
    for room in st.rooms.iter().filter(|r| r.is_member(&st.me) && conversation.as_ref().is_none_or(|c| *c == r.token)) {
        for m in room.messages.iter().filter(|m| m.system.is_empty() && m.deleted_by.is_none()) {
            let mut text = m.text.clone();
            for (key, param) in &m.params {
                let name = param.get("name").and_then(Value::as_str).unwrap_or_default();
                let shown = if param.get("type").and_then(Value::as_str) == Some("user") { format!("@{name}") } else { name.to_string() };
                text = text.replace(&format!("{{{key}}}"), &shown);
            }
            if m.text.to_lowercase().contains(&term) || text.to_lowercase().contains(&term) {
                found.push((room, m, text));
            }
        }
    }
    found.sort_by(|a, b| b.1.ts.cmp(&a.1.ts).then(b.1.id.cmp(&a.1.id)));
    let entries: Vec<Value> = found
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(room, m, text)| {
            let chars: Vec<char> = text.chars().collect();
            let lower = text.to_lowercase();
            let at = lower.find(&term).map_or(0, |b| lower[..b].chars().count()).min(chars.len());
            let subline = if at > 30 && chars.len() > 40 { format!("…{}", chars[at - 10..].iter().collect::<String>()) } else { text };
            let title = match room.kind {
                1 => st.name_of(&m.actor),
                _ => format!("{} in {}", st.name_of(&m.actor), st.room_json(room)["displayName"].as_str().unwrap_or_default()),
            };
            json!({
                "thumbnailUrl": "",
                "title": title,
                "subline": subline,
                "resourceUrl": format!("/call/{}#message_{}", room.token, m.id),
                "icon": "icon-talk",
                "rounded": true,
                "attributes": {
                    "conversation": room.token,
                    "messageId": m.id,
                    "actorType": "users",
                    "actorId": m.actor,
                    "timestamp": m.ts.to_string(),
                },
            })
        })
        .collect();
    ocs(json!({ "name": "Messages", "isPaginated": true, "entries": entries, "cursor": offset + limit }))
}

pub async fn chat_post(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let text = p.str("message").unwrap_or_default();
    if text.trim().is_empty() {
        return bad_request("message");
    }
    if st.room(&token).is_none() {
        return not_found();
    }
    let (text, params) = st.mentions(st.room(&token), &text);
    let mut m = st.new_msg(&st.me.clone(), text);
    m.params = params;
    m.reference_id = p.str("referenceId").unwrap_or_default();
    m.reply_to = p.i64("replyTo").filter(|id| st.room(&token).and_then(|r| r.msg(*id)).is_some());
    let Some(id) = st.push_msg(&token, m) else { return not_found() };
    let r = st.room_mut(&token).unwrap();
    r.last_read = id;
    let r = st.room(&token).unwrap();
    let data = st.msg_json(r, r.msg(id).unwrap(), true);
    ocs_with_headers(StatusCode::CREATED, data, &chat_headers(None, r.common_read()))
}

/// `GET /chat/{token}/mentions`: who can be mentioned. Like Talk: the members, who wrote
/// last first and the user themselves last, then groups and teams, then everybody
/// (`source: calls`, labelled with the conversation's name; not in a one-to-one chat).
/// `limit` is ignored, as on a real server.
pub async fn chat_mentions(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let st = app.st.lock().unwrap();
    let Some(r) = st.room(&token) else { return not_found() };
    let q = p.str("search").unwrap_or_default().to_lowercase();
    let fits = |label: &str, id: &str| label.to_lowercase().contains(&q) || id.to_lowercase().contains(&q);
    let wrote = |person: &str| r.messages.iter().rev().find(|m| m.actor == person && m.system.is_empty()).map_or(0, |m| m.id);
    let mut people: Vec<&str> = r.members.iter().map(|m| m.person.as_str()).collect();
    people.sort_by_key(|person| (*person == st.me, std::cmp::Reverse(wrote(person))));
    let mut out: Vec<Value> = people
        .into_iter()
        .filter(|person| fits(&st.name_of(person), person))
        .map(|person| json!({ "id": person, "label": st.name_of(person), "source": "users", "mentionId": person }))
        .collect();
    for (list, source, prefix) in [(&r.groups, "groups", "group/"), (&r.teams, "teams", "team/")] {
        for c in list.iter().filter(|c| fits(&c.name, &c.id)) {
            let id = format!("{prefix}{}", c.id);
            out.push(json!({ "id": id, "label": c.name, "source": source, "mentionId": id }));
        }
    }
    if r.kind != 1 && (r.name.to_lowercase().contains(&q) || "all".starts_with(&q) || "here".starts_with(&q)) {
        let details = format!("All {} participants", r.members.len());
        out.push(json!({ "id": "all", "label": r.name, "source": "calls", "mentionId": "all", "details": details }));
    }
    ocs(Value::Array(out))
}

/// `POST /chat/{token}/share`: a rich object as a message. Like Talk it wants a name and,
/// for a place, a `geo:` id; the demo knows no other object types.
pub async fn chat_share(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    if st.room(&token).is_none() {
        return not_found();
    }
    let id = p.str("objectId").unwrap_or_default();
    let mut meta: Value = serde_json::from_str(&p.str("metaData").unwrap_or_default()).unwrap_or(Value::Null);
    let named = meta["name"].as_str().is_some_and(|n| !n.is_empty());
    let place = id.strip_prefix("geo:").and_then(|c| c.split_once(',')).is_some_and(|(lat, lon)| {
        lat.parse::<f64>().is_ok_and(|v| v.abs() < 100.0) && lon.parse::<f64>().is_ok_and(|v| v.abs() < 1000.0)
    });
    if p.str("objectType").as_deref() != Some("geo-location") || !named || !place {
        return bad_request("object");
    }
    meta["type"] = json!("geo-location");
    meta["id"] = json!(id);
    let mut m = st.new_msg(&st.me.clone(), "{object}".into());
    m.params.insert("actor".into(), st.user_param(&st.me));
    m.params.insert("object".into(), meta);
    m.reference_id = p.str("referenceId").unwrap_or_default();
    let Some(id) = st.push_msg(&token, m) else { return not_found() };
    st.room_mut(&token).unwrap().last_read = id;
    let r = st.room(&token).unwrap();
    let data = st.msg_json(r, r.msg(id).unwrap(), true);
    ocs_with_headers(StatusCode::CREATED, data, &chat_headers(None, r.common_read()))
}

/// Test hook, not part of Nextcloud: somebody else writes into a conversation, so the
/// signed-in user gets something unread (several accounts: the badge of the account you
/// are not in, docs/accounts.md). `token`, `text` ("@id" mentions) and optionally `from`
/// (a member; default: the first other member). With `lat` and `lon` it is a shared place
/// and `text` its name. With `contact=1` it is a shared contact, `text` the person's name.
pub async fn demo_incoming(AxState(app): S, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let token = p.str("token").unwrap_or_default();
    let text = p.str("text").unwrap_or_default();
    if text.trim().is_empty() {
        return bad_request("text");
    }
    let place = p.str("lat").and_then(|v| v.parse::<f64>().ok()).zip(p.str("lon").and_then(|v| v.parse::<f64>().ok()));
    let Some(room) = st.room(&token) else { return not_found() };
    let from = p
        .str("from")
        .or_else(|| room.members.iter().find(|m| m.person != st.me).map(|m| m.person.clone()))
        .unwrap_or_default();
    if from == st.me || !room.is_member(&from) {
        return bad_request("from");
    }
    let contact = p.str("contact").is_some_and(|v| v != "0" && !v.is_empty());
    let m = match place {
        _ if contact => {
            let name = text.trim().replace(['\r', '\n', ';', ',', '\\', '/'], " ");
            let card = format!(
                "BEGIN:VCARD\r\nVERSION:3.0\r\nN:{name};;;;\r\nFN:{name}\r\nTEL;TYPE=CELL:+49 30 23125 118\r\nEND:VCARD\r\n"
            );
            let file = format!("{name}.vcf");
            let file_id = st.add_file(&file, &format!("Talk/{file}"), std::sync::Arc::new(card.into_bytes()));
            let mut m = st.new_msg(&from, "{file}".into());
            m.params.insert("actor".into(), st.user_param(&from));
            m.params.insert("file".into(), st.file_param(file_id));
            m.file = Some(file_id);
            m
        }
        Some((lat, lon)) => {
            let mut m = st.new_msg(&from, "{object}".into());
            m.params.insert("actor".into(), st.user_param(&from));
            m.params.insert("object".into(), crate::state::location_param(lat, lon, text.trim()));
            m
        }
        None => {
            let (text, params) = st.mentions(st.room(&token), &text);
            let mut m = st.new_msg(&from, text);
            m.params = params;
            m
        }
    };
    let Some(id) = st.push_msg(&token, m) else { return not_found() };
    let r = st.room(&token).unwrap();
    ocs_status(StatusCode::CREATED, st.msg_json(r, r.msg(id).unwrap(), true))
}

pub async fn read_marker(AxState(app): S, Path(token): Path<String>, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let Some(r) = st.room_mut(&token) else { return not_found() };
    let newest = r.newest_id();
    r.last_read = p.i64("lastReadMessage").unwrap_or(newest).min(newest);
    r.touch();
    let r = st.room(&token).unwrap();
    let common = r.common_read();
    ocs_with_headers(StatusCode::OK, st.room_json(r), &chat_headers(None, common))
}

/// Mark unread: the marker goes back before the newest visible message.
pub async fn unread_marker(AxState(app): S, Path(token): Path<String>) -> Response {
    let mut st = app.st.lock().unwrap();
    let Some(r) = st.room_mut(&token) else { return not_found() };
    let visible: Vec<i64> = r.messages.iter().filter(|m| m.visible()).map(|m| m.id).collect();
    r.last_read = visible.iter().rev().nth(1).copied().unwrap_or(0);
    r.touch();
    let r = st.room(&token).unwrap();
    ocs(st.room_json(r))
}

// --- edit and delete ------------------------------------------------------------------------

enum Change {
    Edit(String),
    Delete,
}

fn refused(status: StatusCode, error: &str) -> Response {
    ocs_status(status, json!({ "error": error }))
}

/// Edits or deletes a message as `actor` (`None`: the signed-in user, with the rules of
/// Talk) and posts the hidden system message whose `parent` is the new version. That
/// message is also the answer, with a `parent` as Talk sends it there: without the own
/// reactions.
fn change_message(app: &App, token: &str, id: i64, actor: Option<String>, change: Change) -> Response {
    let mut st = app.st.lock().unwrap();
    let me = st.me.clone();
    let Some(r) = st.room(token) else { return not_found() };
    let Some(m) = r.msg(id) else { return refused(StatusCode::NOT_FOUND, "message") };
    if !m.system.is_empty() || m.deleted_by.is_some() {
        return refused(StatusCode::METHOD_NOT_ALLOWED, "message");
    }
    let by = match actor {
        Some(a) if r.is_member(&a) => a,
        Some(_) => return bad_request("from"),
        None => {
            // Moderators may change what others wrote, but not in a one-to-one chat.
            let moderator = r.members.iter().any(|x| x.person == me && x.ptype <= 2);
            if m.actor != me && !(moderator && r.kind != 1) {
                return refused(StatusCode::FORBIDDEN, "permission");
            }
            me.clone()
        }
    };
    if let Change::Edit(text) = &change {
        if text.trim().is_empty() {
            return bad_request("message");
        }
        if text.chars().count() > 32000 {
            return ocs_status(StatusCode::PAYLOAD_TOO_LARGE, json!([]));
        }
        if m.ts < now() - 24 * 60 * 60 {
            return bad_request("age");
        }
    }
    let system = match &change {
        Change::Edit(text) => {
            let (text, mut params) = st.mentions(st.room(token), text.trim());
            let m = st.room_mut(token).unwrap().messages.iter_mut().find(|m| m.id == id).unwrap();
            // The file and the author of a file message stay; the text is its caption.
            for key in ["actor", "file"] {
                if let Some(v) = m.params.get(key) {
                    params.insert(key.into(), v.clone());
                }
            }
            m.text = text;
            m.params = params;
            m.edited = Some((by.clone(), now()));
            "message_edited"
        }
        Change::Delete => {
            let m = st.room_mut(token).unwrap().messages.iter_mut().find(|m| m.id == id).unwrap();
            m.deleted_by = Some(by.clone());
            m.reactions.clear();
            "message_deleted"
        }
    };
    let r = st.room(token).unwrap();
    let snapshot = st.msg_json(r, r.msg(id).unwrap(), false);
    let verb = if system == "message_edited" { "bearbeitet" } else { "gelöscht" };
    let text = st.system_text(&by, &format!("hast eine Nachricht {verb}"), &format!("hat eine Nachricht {verb}"));
    let mut sys = st.system_msg(system, text);
    sys.actor = by.clone();
    sys.params.insert("actor".into(), st.user_param(&by));
    sys.parent_snapshot = Some(snapshot);
    let Some(sys_id) = st.push_quiet(token, sys) else { return not_found() };
    let r = st.room(token).unwrap();
    let mut data = st.msg_json(r, r.msg(sys_id).unwrap(), true);
    if let Some(parent) = data.get_mut("parent").and_then(Value::as_object_mut) {
        parent.remove("reactionsSelf");
    }
    ocs_with_headers(StatusCode::OK, data, &chat_headers(None, r.common_read()))
}

pub async fn chat_edit(AxState(app): S, Path((token, id)): Path<(String, i64)>, p: Params) -> Response {
    change_message(&app, &token, id, None, Change::Edit(p.str("message").unwrap_or_default()))
}

pub async fn chat_delete(AxState(app): S, Path((token, id)): Path<(String, i64)>) -> Response {
    change_message(&app, &token, id, None, Change::Delete)
}

/// Test hook: the author of a message edits it. `token`, `id`, `text`.
pub async fn demo_edit(AxState(app): S, p: Params) -> Response {
    let (token, id) = (p.str("token").unwrap_or_default(), p.i64("id").unwrap_or(0));
    let Some(author) = author_of(&app, &token, id) else { return not_found() };
    change_message(&app, &token, id, Some(author), Change::Edit(p.str("text").unwrap_or_default()))
}

/// Test hook: the author of a message deletes it. `token`, `id`.
pub async fn demo_delete(AxState(app): S, p: Params) -> Response {
    let (token, id) = (p.str("token").unwrap_or_default(), p.i64("id").unwrap_or(0));
    let Some(author) = author_of(&app, &token, id) else { return not_found() };
    change_message(&app, &token, id, Some(author), Change::Delete)
}

fn author_of(app: &App, token: &str, id: i64) -> Option<String> {
    let st = app.st.lock().unwrap();
    Some(st.room(token)?.msg(id)?.actor.clone())
}

// --- reactions ------------------------------------------------------------------------------

fn reactable(st: &State, token: &str, id: i64) -> Result<(), Box<Response>> {
    let Some(r) = st.room(token) else { return Err(Box::new(not_found())) };
    match r.msg(id) {
        Some(m) if m.system.is_empty() && m.deleted_by.is_none() => Ok(()),
        Some(_) => Err(Box::new(bad_request("message"))),
        None => Err(Box::new(not_found())),
    }
}

pub async fn reactions_get(AxState(app): S, Path((token, id)): Path<(String, i64)>, p: Params) -> Response {
    let st = app.st.lock().unwrap();
    if let Err(e) = reactable(&st, &token, id) {
        return *e;
    }
    let m = st.room(&token).unwrap().msg(id).unwrap();
    ocs(st.reactions_json(m, p.str("reaction").as_deref()))
}

/// Adds or removes the reaction of `actor` (`None`: the signed-in user) and posts the
/// hidden `reaction` system message whose `parent` is the updated message (that is how the
/// other clients learn about it).
fn change_reaction(app: &App, token: &str, id: i64, emoji: &str, add: bool, actor: Option<String>) -> Response {
    let mut st = app.st.lock().unwrap();
    if let Err(e) = reactable(&st, token, id) {
        return *e;
    }
    if emoji.is_empty() {
        return bad_request("reaction");
    }
    let me = actor.unwrap_or_else(|| st.me.clone());
    let r = st.room_mut(token).unwrap();
    let m = r.messages.iter_mut().find(|m| m.id == id).unwrap();
    let had = m.reactions.iter().any(|x| x.actor == me && x.emoji == emoji);
    let status = match (add, had) {
        (true, true) => StatusCode::OK,
        (true, false) => {
            m.reactions.push(Reaction { emoji: emoji.into(), actor: me.clone(), ts: now() });
            StatusCode::CREATED
        }
        (false, true) => {
            m.reactions.retain(|x| !(x.actor == me && x.emoji == emoji));
            StatusCode::OK
        }
        (false, false) => return not_found(),
    };
    if !(add && had) {
        let r = st.room(token).unwrap();
        let snapshot = st.msg_json(r, r.msg(id).unwrap(), false);
        let mut sys = st.system_msg(if add { "reaction" } else { "reaction_deleted" }, emoji.into());
        sys.params.insert("actor".into(), st.user_param(&me));
        sys.actor = me.clone();
        sys.parent_snapshot = Some(snapshot);
        // Talk wakes the long polls for a reaction taken back, not for a new one.
        if add {
            st.push_quietly(token, sys);
        } else {
            st.push_msg(token, sys);
        }
    }
    let m = st.room(token).unwrap().msg(id).unwrap();
    ocs_status(status, st.reactions_json(m, None))
}

pub async fn reaction_add(AxState(app): S, Path((token, id)): Path<(String, i64)>, p: Params) -> Response {
    change_reaction(&app, &token, id, &p.str("reaction").unwrap_or_default(), true, None)
}

/// Test hook: somebody else reacts to a message, or takes the reaction back with
/// `remove=1`. `token`, `id`, `emoji` and optionally `from` (a member; default: the first
/// other member).
pub async fn demo_react(AxState(app): S, p: Params) -> Response {
    let (token, id) = (p.str("token").unwrap_or_default(), p.i64("id").unwrap_or(0));
    let from = {
        let st = app.st.lock().unwrap();
        let Some(room) = st.room(&token) else { return not_found() };
        let from = p
            .str("from")
            .or_else(|| room.members.iter().find(|m| m.person != st.me).map(|m| m.person.clone()))
            .unwrap_or_default();
        if from == st.me || !room.is_member(&from) {
            return bad_request("from");
        }
        from
    };
    let add = !p.bool("remove").unwrap_or(false);
    change_reaction(&app, &token, id, &p.str("emoji").unwrap_or_default(), add, Some(from))
}

pub async fn reaction_remove(AxState(app): S, Path((token, id)): Path<(String, i64)>, p: Params) -> Response {
    change_reaction(&app, &token, id, &p.str("reaction").unwrap_or_default(), false, None)
}

// --- categories (conversation tags) ------------------------------------------------------------

fn sorted_tags(st: &State) -> Value {
    let mut tags = st.tags.clone();
    tags.sort_by_key(|t| t.sort_order);
    Value::Array(tags.iter().map(Tag::json).collect())
}

pub async fn tags(AxState(app): S) -> Response {
    ocs(sorted_tags(&app.st.lock().unwrap()))
}

pub async fn create_tag(AxState(app): S, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let name = p.str("name").unwrap_or_default().trim().to_string();
    if name.is_empty() || name.chars().count() > 250 || st.tags.iter().any(|t| t.name == name) {
        return bad_request("name");
    }
    let id = st.next_tag.to_string();
    st.next_tag += 1;
    let order = st.tags.iter().map(|t| t.sort_order).max().unwrap_or(0) + 1;
    let tag = Tag { id, name, kind: "custom".into(), sort_order: order, collapsed: false };
    st.tags.push(tag.clone());
    ocs(tag.json())
}

pub async fn rename_tag(AxState(app): S, Path(id): Path<String>, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let name = p.str("name").unwrap_or_default().trim().to_string();
    if name.is_empty() {
        return bad_request("name");
    }
    let Some(t) = st.tags.iter_mut().find(|t| t.id == id) else { return not_found() };
    if t.kind != "custom" {
        return bad_request("type");
    }
    t.name = name;
    ocs(t.json())
}

pub async fn delete_tag(AxState(app): S, Path(id): Path<String>) -> Response {
    let mut st = app.st.lock().unwrap();
    match st.tags.iter().find(|t| t.id == id) {
        None => return not_found(),
        Some(t) if t.kind != "custom" => return bad_request("type"),
        _ => {}
    }
    st.tags.retain(|t| t.id != id);
    for r in st.rooms.iter_mut() {
        if r.tag_ids.contains(&id) {
            r.tag_ids.retain(|t| *t != id);
            r.touch();
        }
    }
    ocs(json!([]))
}

pub async fn reorder_tags(AxState(app): S, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    for (i, id) in p.list("orderedIds").iter().enumerate() {
        if let Some(t) = st.tags.iter_mut().find(|t| t.id == *id) {
            t.sort_order = i as i64;
        }
    }
    ocs(sorted_tags(&st))
}

pub async fn collapse_tag(AxState(app): S, Path(id): Path<String>, p: Params) -> Response {
    let mut st = app.st.lock().unwrap();
    let collapsed = p.bool("collapsed").unwrap_or(false);
    let Some(t) = st.tags.iter_mut().find(|t| t.id == id) else { return not_found() };
    t.collapsed = collapsed;
    ocs(t.json())
}

// --- anything else ------------------------------------------------------------------------------

/// Endpoints we do not know: logged (to see what is missing) and answered with an empty
/// OCS success, which the clients treat as "nothing there".
pub async fn unknown(method: axum::http::Method, uri: axum::http::Uri, headers: HeaderMap) -> Response {
    let _ = headers;
    tracing::warn!("nicht implementiert: {method} {uri}");
    ocs(json!([]))
}
