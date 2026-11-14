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

pub async fn capabilities() -> Response {
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
                    "mention-flag", "last-room-activity", "avatar", "conversation-creation-all"
                ],
                "config": {
                    "attachments": { "allowed": true, "folder": "/Talk" },
                    "chat": { "max-length": 32000, "read-privacy": 0, "typing-privacy": 1, "has-translation-providers": false },
                    "conversations": { "can-create": true, "force-passwords": false },
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

pub async fn notifications() -> Response {
    ocs(json!([]))
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

/// History (`lookIntoFuture=0`, newest first) or long poll (`lookIntoFuture=1`: waits up
/// to `timeout` seconds for new messages, then 304).
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
            let picked: Vec<&crate::state::Msg> = if future {
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
            rx
        };
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return StatusCode::NOT_MODIFIED.into_response();
        }
        if tokio::time::timeout(left, rx.changed()).await.is_err() {
            return StatusCode::NOT_MODIFIED.into_response();
        }
    }
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
    let (text, params) = st.mentions(&text);
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

// --- reactions ------------------------------------------------------------------------------

fn reactable(st: &State, token: &str, id: i64) -> Result<(), Box<Response>> {
    let Some(r) = st.room(token) else { return Err(Box::new(not_found())) };
    match r.msg(id) {
        Some(m) if m.system.is_empty() => Ok(()),
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

/// Adds or removes our reaction and posts the hidden `reaction` system message whose
/// `parent` is the updated message (that is how the other clients learn about it).
fn change_reaction(app: &App, token: &str, id: i64, emoji: &str, add: bool) -> Response {
    let mut st = app.st.lock().unwrap();
    if let Err(e) = reactable(&st, token, id) {
        return *e;
    }
    if emoji.is_empty() {
        return bad_request("reaction");
    }
    let me = st.me.clone();
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
        sys.parent_snapshot = Some(snapshot);
        st.push_msg(token, sys);
    }
    let m = st.room(token).unwrap().msg(id).unwrap();
    ocs_status(status, st.reactions_json(m, None))
}

pub async fn reaction_add(AxState(app): S, Path((token, id)): Path<(String, i64)>, p: Params) -> Response {
    change_reaction(&app, &token, id, &p.str("reaction").unwrap_or_default(), true)
}

pub async fn reaction_remove(AxState(app): S, Path((token, id)): Path<(String, i64)>, p: Params) -> Response {
    change_reaction(&app, &token, id, &p.str("reaction").unwrap_or_default(), false)
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
