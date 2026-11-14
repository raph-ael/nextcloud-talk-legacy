//! In-memory model of the demo server: people, conversations, messages, files and
//! categories, loaded from `fixtures/` with timestamps relative to the server start.
//! Everything the clients change (sent messages, reactions, read markers) lives here
//! until the server stops.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::{Duration, Local, NaiveTime, TimeZone};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::watch;

/// Others "read" a new message this many seconds after it was sent (read receipts).
const READ_DELAY_S: i64 = 5;
/// Permissions of the current user: everything up to chat (128) and react (256).
const PERMISSIONS: i64 = 510;
/// Hidden in the clients, never counted or shown as last message.
const HIDDEN: &[&str] = &["reaction", "reaction_deleted", "reaction_revoked"];

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub struct Person {
    pub id: String,
    pub name: String,
    pub avatar: Option<Arc<Vec<u8>>>,
}

pub struct File {
    pub id: i64,
    pub name: String,
    /// Below the user root without leading slash, e.g. `Talk/Hochbeet.jpg`.
    pub path: String,
    pub mime: String,
    pub data: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

pub struct Reaction {
    pub emoji: String,
    pub actor: String,
    pub ts: i64,
}

pub struct Msg {
    pub id: i64,
    pub actor: String,
    pub ts: i64,
    pub text: String,
    pub params: Map<String, Value>,
    pub system: String,
    /// `comment`, `system` or `voice-message`.
    pub kind: String,
    pub reference_id: String,
    pub reply_to: Option<i64>,
    pub reactions: Vec<Reaction>,
    pub file: Option<i64>,
    /// Reaction system messages carry the updated message as `parent`.
    pub parent_snapshot: Option<Value>,
}

impl Msg {
    pub fn visible(&self) -> bool {
        !HIDDEN.contains(&self.system.as_str())
    }

    fn mentions(&self, user: &str) -> bool {
        self.params
            .values()
            .any(|p| p["type"] == "user" && p["id"] == user && p.get("mention-id").is_some())
    }
}

pub struct Member {
    pub person: String,
    /// 1 owner, 2 moderator, 3 user.
    pub ptype: u8,
    pub attendee_id: i64,
}

pub struct Room {
    pub id: i64,
    pub token: String,
    /// 1 one-to-one, 2 group.
    pub kind: u8,
    pub name: String,
    pub description: String,
    pub members: Vec<Member>,
    pub favorite: bool,
    pub archived: bool,
    pub notification_level: u8,
    pub tag_ids: Vec<String>,
    pub avatar: Option<Arc<Vec<u8>>>,
    pub avatar_version: String,
    pub custom_avatar: bool,
    pub last_read: i64,
    pub messages: Vec<Msg>,
    pub modified: i64,
    pub created: i64,
    /// Bumped on every change: wakes the long polls of this room.
    pub tx: watch::Sender<u64>,
}

impl Room {
    pub fn is_member(&self, person: &str) -> bool {
        self.members.iter().any(|m| m.person == person)
    }

    pub fn touch(&mut self) {
        self.modified = now();
        self.tx.send_modify(|v| *v += 1);
    }

    pub fn msg(&self, id: i64) -> Option<&Msg> {
        self.messages.iter().find(|m| m.id == id)
    }

    /// Highest id everybody else has read: all but the newest few seconds.
    pub fn common_read(&self) -> i64 {
        let limit = now() - READ_DELAY_S;
        self.messages
            .iter()
            .filter(|m| m.ts <= limit)
            .map(|m| m.id)
            .max()
            .unwrap_or(0)
    }

    pub fn newest_id(&self) -> i64 {
        self.messages.last().map_or(0, |m| m.id)
    }
}

#[derive(Clone)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub sort_order: i64,
    pub collapsed: bool,
}

impl Tag {
    pub fn json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "sortOrder": self.sort_order,
            "collapsed": self.collapsed,
            "type": self.kind,
        })
    }
}

pub struct LoginFlow {
    pub base: String,
    pub granted: bool,
}

pub struct State {
    pub me: String,
    pub people: Vec<Person>,
    pub rooms: Vec<Room>,
    pub files: HashMap<i64, File>,
    /// Uploaded files (WebDAV PUT), by path with leading slash.
    pub uploads: HashMap<String, Arc<Vec<u8>>>,
    /// Files shared into conversations: (path with leading slash, token).
    pub shares: Vec<(String, String)>,
    pub tags: Vec<Tag>,
    pub login_flows: HashMap<String, LoginFlow>,
    pub next_msg: i64,
    pub next_file: i64,
    pub next_room: i64,
    pub next_tag: i64,
}

// --- fixtures --------------------------------------------------------------------------

#[derive(Deserialize)]
struct PeopleFile {
    me: String,
    people: Vec<PersonFx>,
}

#[derive(Deserialize)]
struct PersonFx {
    id: String,
    name: String,
    #[serde(default)]
    avatar: Option<String>,
}

#[derive(Deserialize)]
struct ConversationsFile {
    #[serde(default)]
    categories: Vec<CategoryFx>,
    rooms: Vec<RoomFx>,
}

#[derive(Deserialize)]
struct CategoryFx {
    id: String,
    name: String,
    #[serde(default)]
    rooms: Vec<String>,
}

#[derive(Deserialize)]
struct RoomFx {
    token: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    avatar: Option<String>,
    /// One-to-one: the other person.
    #[serde(default)]
    with: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    favorite: bool,
    #[serde(default)]
    unread: usize,
    messages: Vec<MsgFx>,
}

#[derive(Deserialize)]
struct MsgFx {
    #[serde(default)]
    key: Option<String>,
    at: String,
    from: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    system: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    reply: Option<String>,
    #[serde(default)]
    reactions: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(default)]
    file: Option<FileFx>,
}

#[derive(Deserialize)]
struct FileFx {
    asset: String,
    name: String,
    #[serde(default)]
    voice: bool,
}

/// `-2 18:30` (two days ago at 18:30 local time), `-45m`, `-3h`, `-2d`.
pub fn parse_at(at: &str, start: i64) -> Result<i64, String> {
    let at = at.trim();
    if let Some((day, time)) = at.split_once(' ') {
        let day: i64 = day.parse().map_err(|_| format!("Tag in \"{at}\""))?;
        let time = NaiveTime::parse_from_str(time.trim(), "%H:%M").map_err(|_| format!("Uhrzeit in \"{at}\""))?;
        let date = Local
            .timestamp_opt(start, 0)
            .single()
            .ok_or("Zeit")?
            .date_naive()
            + Duration::days(day);
        let local = Local
            .from_local_datetime(&date.and_time(time))
            .earliest()
            .ok_or_else(|| format!("Ortszeit in \"{at}\""))?;
        return Ok(local.timestamp());
    }
    let (num, unit) = at.split_at(at.len().saturating_sub(1));
    let n: i64 = num.trim_start_matches('-').parse().map_err(|_| format!("Zeitangabe \"{at}\""))?;
    let secs = match unit {
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        _ => return Err(format!("Einheit in \"{at}\" (m, h oder d)")),
    };
    Ok(start - secs)
}

/// FNV-1a: stable short version strings for avatars.
fn version_of(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:012x}", h & 0xffff_ffff_ffff)
}

pub fn mime_for(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

pub fn image_size(data: &[u8]) -> (u32, u32) {
    image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .unwrap_or((0, 0))
}

impl State {
    pub fn load(dir: &Path) -> Result<State, String> {
        let start = now();
        let read = |rel: &str| std::fs::read(dir.join(rel)).map_err(|e| format!("{}: {e}", dir.join(rel).display()));
        let people: PeopleFile = serde_json::from_slice(&read("people.json")?).map_err(|e| format!("people.json: {e}"))?;
        let convs: ConversationsFile =
            serde_json::from_slice(&read("conversations.json")?).map_err(|e| format!("conversations.json: {e}"))?;

        let mut st = State {
            me: people.me.clone(),
            people: Vec::new(),
            rooms: Vec::new(),
            files: HashMap::new(),
            uploads: HashMap::new(),
            shares: Vec::new(),
            tags: Vec::new(),
            login_flows: HashMap::new(),
            next_msg: 100,
            next_file: 2001,
            next_room: 1,
            next_tag: 10,
        };
        for p in people.people {
            let avatar = match &p.avatar {
                Some(a) => Some(Arc::new(read(a)?)),
                None => None,
            };
            st.people.push(Person { id: p.id, name: p.name, avatar });
        }
        if st.person(&st.me).is_none() {
            return Err(format!("\"me\" ({}) fehlt in people.json", st.me));
        }

        // Built-in categories plus the fixture ones (docs/categories.md).
        st.tags.push(Tag { id: "1".into(), name: String::new(), kind: "favorites".into(), sort_order: 0, collapsed: false });
        for (i, c) in convs.categories.iter().enumerate() {
            st.tags.push(Tag {
                id: c.id.clone(),
                name: c.name.clone(),
                kind: "custom".into(),
                sort_order: i as i64 + 1,
                collapsed: false,
            });
        }
        st.tags.push(Tag {
            id: "2".into(),
            name: String::new(),
            kind: "other".into(),
            sort_order: convs.categories.len() as i64 + 1,
            collapsed: false,
        });

        for fx in convs.rooms {
            let token = fx.token.clone();
            let room = st
                .load_room(fx, &convs.categories, dir, start)
                .map_err(|e| format!("Unterhaltung {token}: {e}"))?;
            st.rooms.push(room);
        }
        Ok(st)
    }

    fn load_room(&mut self, fx: RoomFx, categories: &[CategoryFx], dir: &Path, start: i64) -> Result<Room, String> {
        let me = self.me.clone();
        let (kind, members, avatar, custom_avatar, name) = match fx.kind.as_str() {
            "one-to-one" => {
                let other = fx.with.clone().ok_or("\"with\" fehlt")?;
                let person = self.person(&other).ok_or_else(|| format!("unbekannte Person {other}"))?;
                (1u8, vec![me.clone(), other.clone()], person.avatar.clone(), false, other)
            }
            "group" => {
                let mut members = fx.members.clone();
                if !members.contains(&me) {
                    members.insert(0, me.clone());
                }
                let avatar = match &fx.avatar {
                    Some(a) => Some(Arc::new(
                        std::fs::read(dir.join(a)).map_err(|e| format!("{a}: {e}"))?,
                    )),
                    None => None,
                };
                let custom = avatar.is_some();
                (2u8, members, avatar, custom, fx.name.clone())
            }
            other => return Err(format!("Typ {other} (one-to-one oder group)")),
        };
        for m in &members {
            if self.person(m).is_none() {
                return Err(format!("unbekanntes Mitglied {m}"));
            }
        }
        let id = self.next_room;
        self.next_room += 1;
        let members = members
            .into_iter()
            .enumerate()
            .map(|(i, person)| Member {
                ptype: if kind == 1 || fx.owner.as_deref() == Some(person.as_str()) { 1 } else { 3 },
                attendee_id: id * 100 + i as i64 + 1,
                person,
            })
            .collect();

        let mut room = Room {
            id,
            token: fx.token.clone(),
            kind,
            name,
            description: fx.description.clone(),
            members,
            favorite: fx.favorite,
            archived: false,
            notification_level: 0,
            tag_ids: categories
                .iter()
                .filter(|c| c.rooms.contains(&fx.token))
                .map(|c| c.id.clone())
                .collect(),
            avatar_version: avatar.as_deref().map(|a| version_of(a)).unwrap_or_else(|| "0".into()),
            avatar,
            custom_avatar,
            last_read: 0,
            messages: Vec::new(),
            modified: start,
            created: start,
            tx: watch::channel(0).0,
        };

        let mut keys: HashMap<String, i64> = HashMap::new();
        let mut prev_ts = 0;
        for m in fx.messages {
            let actor = self.person(&m.from).ok_or_else(|| format!("unbekannter Absender {}", m.from))?.id.clone();
            if !room.is_member(&actor) {
                return Err(format!("{actor} ist kein Mitglied"));
            }
            let mut ts = parse_at(&m.at, start)?.min(start);
            if ts <= prev_ts {
                ts = prev_ts + 1;
            }
            prev_ts = ts;
            let id = self.next_msg;
            self.next_msg += 1;
            if let Some(k) = &m.key {
                keys.insert(k.clone(), id);
            }
            let reply_to = match &m.reply {
                Some(k) => Some(*keys.get(k).ok_or_else(|| format!("reply auf unbekannten key {k}"))?),
                None => None,
            };
            let reactions = m
                .reactions
                .iter()
                .flat_map(|(emoji, who)| who.iter().map(move |w| (emoji.clone(), w.clone())))
                .map(|(emoji, actor)| Reaction { emoji, actor, ts })
                .collect();

            let mut msg = Msg {
                id,
                actor: actor.clone(),
                ts,
                text: String::new(),
                params: Map::new(),
                system: String::new(),
                kind: "comment".into(),
                reference_id: String::new(),
                reply_to,
                reactions,
                file: None,
                parent_snapshot: None,
            };
            if let Some(sys) = &m.system {
                msg.kind = "system".into();
                msg.system = sys.clone();
                msg.params.insert("actor".into(), self.user_param(&actor));
                msg.text = match (m.text.clone(), sys.as_str()) {
                    (Some(t), _) => t,
                    (None, "conversation_created") => self.system_text(&actor, "hast die Unterhaltung erstellt", "hat die Unterhaltung erstellt"),
                    (None, "user_added") => {
                        let user = m.user.clone().ok_or("user_added braucht \"user\"")?;
                        msg.params.insert("user".into(), self.user_param(&user));
                        self.added_text(&actor, &user)
                    }
                    (None, other) => return Err(format!("system {other} braucht \"text\"")),
                };
            } else if let Some(f) = &m.file {
                let data = Arc::new(std::fs::read(dir.join(&f.asset)).map_err(|e| format!("{}: {e}", f.asset))?);
                let file_id = self.add_file(&f.name, &format!("Talk/{}", f.name), data);
                msg.file = Some(file_id);
                if f.voice {
                    msg.kind = "voice-message".into();
                }
                msg.params.insert("actor".into(), self.user_param(&actor));
                msg.params.insert("file".into(), self.file_param(file_id));
                msg.text = match &m.text {
                    Some(caption) => caption.clone(),
                    None => "{file}".into(),
                };
            } else {
                let text = m.text.clone().ok_or("Nachricht ohne text, file oder system")?;
                let (text, params) = self.mentions(&text);
                msg.text = text;
                msg.params = params;
            }
            room.messages.push(msg);
        }

        // Read marker: everything except the last `unread` messages of the others.
        let mut left = fx.unread;
        room.last_read = room.newest_id();
        if left > 0 {
            for m in room.messages.iter().rev() {
                room.last_read = m.id - 1;
                if m.actor != me && m.system.is_empty() {
                    left -= 1;
                    if left == 0 {
                        break;
                    }
                }
            }
            // The marker sits on the message before the first unread one.
            let first_unread = room.last_read + 1;
            room.last_read = room.messages.iter().filter(|m| m.id < first_unread).map(|m| m.id).max().unwrap_or(0);
        }
        room.created = room.messages.first().map_or(start, |m| m.ts);
        Ok(room)
    }

    pub fn add_file(&mut self, name: &str, path: &str, data: Arc<Vec<u8>>) -> i64 {
        let mime = mime_for(name).to_string();
        let (width, height) = if mime.starts_with("image/") { image_size(&data) } else { (0, 0) };
        let id = self.next_file;
        self.next_file += 1;
        self.files.insert(
            id,
            File { id, name: name.into(), path: path.trim_start_matches('/').into(), mime, data, width, height },
        );
        id
    }

    pub fn person(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }

    pub fn name_of(&self, id: &str) -> String {
        self.person(id).map_or_else(|| id.to_string(), |p| p.name.clone())
    }

    pub fn room(&self, token: &str) -> Option<&Room> {
        self.rooms.iter().find(|r| r.token == token && r.is_member(&self.me))
    }

    pub fn room_mut(&mut self, token: &str) -> Option<&mut Room> {
        let me = self.me.clone();
        self.rooms.iter_mut().find(|r| r.token == token && r.is_member(&me))
    }

    pub fn user_param(&self, id: &str) -> Value {
        json!({ "type": "user", "id": id, "name": self.name_of(id) })
    }

    pub fn system_text(&self, actor: &str, own: &str, other: &str) -> String {
        if actor == self.me {
            format!("Du {own}")
        } else {
            format!("{} {other}", self.name_of(actor))
        }
    }

    pub fn added_text(&self, actor: &str, user: &str) -> String {
        let who = if user == self.me { "dich".to_string() } else { self.name_of(user) };
        if actor == self.me {
            format!("Du hast {who} hinzugefügt")
        } else {
            format!("{} hat {who} hinzugefügt", self.name_of(actor))
        }
    }

    pub fn file_param(&self, file_id: i64) -> Value {
        let Some(f) = self.files.get(&file_id) else { return Value::Null };
        let image = f.mime.starts_with("image/");
        let mut p = json!({
            "type": "file",
            "id": f.id.to_string(),
            "name": f.name,
            "size": f.data.len().to_string(),
            "path": f.path,
            "link": format!("/index.php/f/{}", f.id),
            "etag": version_of(&f.data),
            "permissions": "27",
            "mimetype": f.mime,
            "preview-available": if image { "yes" } else { "no" },
        });
        if image && f.width > 0 {
            p["width"] = f.width.to_string().into();
            p["height"] = f.height.to_string().into();
        }
        p
    }

    /// `@id` of a known person → `{mention-userN}` with its rich object parameter.
    pub fn mentions(&self, text: &str) -> (String, Map<String, Value>) {
        let mut params = Map::new();
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(pos) = rest.find('@') {
            out.push_str(&rest[..pos]);
            let after = &rest[pos + 1..];
            let len = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'))
                .unwrap_or(after.len());
            let id = after[..len].trim_end_matches('.');
            match self.person(id) {
                Some(p) if !id.is_empty() => {
                    let key = format!("mention-user{}", params.len() + 1);
                    params.insert(key.clone(), json!({ "type": "user", "id": p.id, "name": p.name, "mention-id": p.id }));
                    out.push_str(&format!("{{{key}}}"));
                    rest = &after[id.len()..];
                }
                _ => {
                    out.push('@');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        (out, params)
    }

    // --- JSON as Talk sends it -----------------------------------------------------------

    pub fn msg_json(&self, room: &Room, m: &Msg, with_parent: bool) -> Value {
        let mut counts: Map<String, Value> = Map::new();
        let mut own = Vec::new();
        for r in &m.reactions {
            let n = counts.get(&r.emoji).and_then(Value::as_i64).unwrap_or(0) + 1;
            counts.insert(r.emoji.clone(), n.into());
            if r.actor == self.me && !own.contains(&r.emoji) {
                own.push(r.emoji.clone());
            }
        }
        let mut v = json!({
            "id": m.id,
            "token": room.token,
            "actorType": "users",
            "actorId": m.actor,
            "actorDisplayName": self.name_of(&m.actor),
            "timestamp": m.ts,
            "message": m.text,
            // PHP sends empty objects as [].
            "messageParameters": if m.params.is_empty() { json!([]) } else { Value::Object(m.params.clone()) },
            "systemMessage": m.system,
            "messageType": m.kind,
            "isReplyable": m.system.is_empty(),
            "referenceId": m.reference_id,
            "reactions": if counts.is_empty() { json!([]) } else { Value::Object(counts) },
            "expirationTimestamp": 0,
            "markdown": m.system.is_empty(),
            "silent": false,
            "threadId": 0,
        });
        if !own.is_empty() {
            v["reactionsSelf"] = json!(own);
        }
        if with_parent {
            if let Some(p) = &m.parent_snapshot {
                v["parent"] = p.clone();
            } else if let Some(p) = m.reply_to.and_then(|id| room.msg(id)) {
                v["parent"] = self.msg_json(room, p, false);
            }
        }
        v
    }

    pub fn room_json(&self, r: &Room) -> Value {
        let me_member = r.members.iter().find(|m| m.person == self.me);
        let ptype = me_member.map_or(3, |m| m.ptype);
        let visible: Vec<&Msg> = r.messages.iter().filter(|m| m.visible()).collect();
        let unread: Vec<&&Msg> = visible
            .iter()
            .filter(|m| m.id > r.last_read && m.actor != self.me && m.system.is_empty())
            .collect();
        let mention = unread.iter().any(|m| m.mentions(&self.me));
        let last = visible.last();
        let (name, display) = if r.kind == 1 {
            let other = r.members.iter().find(|m| m.person != self.me).map(|m| m.person.clone()).unwrap_or_default();
            (other.clone(), self.name_of(&other))
        } else {
            (r.name.clone(), r.name.clone())
        };
        json!({
            "id": r.id,
            "token": r.token,
            "type": r.kind,
            "name": name,
            "displayName": display,
            "description": r.description,
            "objectType": "",
            "objectId": "",
            "actorType": "users",
            "actorId": self.me,
            "attendeeId": me_member.map_or(0, |m| m.attendee_id),
            "participantType": ptype,
            "permissions": PERMISSIONS,
            "attendeePermissions": 0,
            "callPermissions": 0,
            "defaultPermissions": 0,
            "hasPassword": false,
            "hasCall": false,
            "callFlag": 0,
            "canStartCall": true,
            "canDeleteConversation": r.kind != 1 && ptype == 1,
            "canLeaveConversation": r.kind == 1 || ptype != 1 || r.members.iter().filter(|m| m.ptype <= 2).count() > 1,
            "lastActivity": last.map_or(r.created, |m| m.ts),
            "isFavorite": r.favorite,
            "isArchived": r.archived,
            "notificationLevel": r.notification_level,
            "lobbyState": 0,
            "lobbyTimer": 0,
            "sipEnabled": 0,
            "canEnableSIP": false,
            "unreadMessages": unread.len(),
            "unreadMention": mention,
            "unreadMentionDirect": mention,
            "lastReadMessage": r.last_read,
            "lastCommonReadMessage": r.common_read(),
            "lastMessage": last.map_or(json!([]), |m| self.msg_json(r, m, false)),
            "readOnly": 0,
            "listable": 0,
            "messageExpiration": 0,
            "avatarVersion": r.avatar_version,
            "isCustomAvatar": r.custom_avatar,
            "breakoutRoomMode": 0,
            "breakoutRoomStatus": 0,
            "recordingConsent": 0,
            "mentionPermissions": 0,
            "callRecording": 0,
            "callStartTime": 0,
            "sessionId": "0",
            "lastPing": 0,
            "isImportant": false,
            "isSensitive": false,
            "tagIds": r.tag_ids,
        })
    }

    pub fn participants_json(&self, r: &Room) -> Value {
        Value::Array(
            r.members
                .iter()
                .map(|m| {
                    json!({
                        "roomToken": r.token,
                        "attendeeId": m.attendee_id,
                        "actorType": "users",
                        "actorId": m.person,
                        "displayName": self.name_of(&m.person),
                        "participantType": m.ptype,
                        "permissions": PERMISSIONS,
                        "attendeePermissions": 0,
                        "sessionIds": [],
                        "inCall": 0,
                        "lastPing": 0,
                    })
                })
                .collect(),
        )
    }

    /// `{emoji: [{actorType, actorId, actorDisplayName, timestamp}]}`, `[]` when empty.
    pub fn reactions_json(&self, m: &Msg, only: Option<&str>) -> Value {
        let mut map: Map<String, Value> = Map::new();
        for r in m.reactions.iter().filter(|r| only.is_none_or(|e| e == r.emoji)) {
            let entry = map.entry(r.emoji.clone()).or_insert_with(|| json!([]));
            entry.as_array_mut().unwrap().push(json!({
                "actorType": "users",
                "actorId": r.actor,
                "actorDisplayName": self.name_of(&r.actor),
                "timestamp": r.ts,
            }));
        }
        if map.is_empty() { json!([]) } else { Value::Object(map) }
    }

    /// Appends a message to a room (new id, now) and wakes its long polls.
    pub fn push_msg(&mut self, token: &str, mut msg: Msg) -> Option<i64> {
        let id = self.next_msg;
        self.next_msg += 1;
        msg.id = id;
        let room = self.rooms.iter_mut().find(|r| r.token == token)?;
        msg.ts = msg.ts.max(room.messages.last().map_or(0, |m| m.ts));
        room.messages.push(msg);
        room.touch();
        Some(id)
    }

    pub fn new_msg(&self, actor: &str, text: String) -> Msg {
        Msg {
            id: 0,
            actor: actor.into(),
            ts: now(),
            text,
            params: Map::new(),
            system: String::new(),
            kind: "comment".into(),
            reference_id: String::new(),
            reply_to: None,
            reactions: Vec::new(),
            file: None,
            parent_snapshot: None,
        }
    }

    pub fn system_msg(&self, system: &str, text: String) -> Msg {
        let mut m = self.new_msg(&self.me, text);
        m.kind = "system".into();
        m.system = system.into();
        m.params.insert("actor".into(), self.user_param(&self.me));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_times() {
        let start = 1_700_000_000;
        assert_eq!(parse_at("-45m", start).unwrap(), start - 2700);
        assert_eq!(parse_at("-3h", start).unwrap(), start - 10800);
        let t = parse_at("-1 18:30", start).unwrap();
        let local = Local.timestamp_opt(t, 0).unwrap();
        assert_eq!(local.format("%H:%M").to_string(), "18:30");
        assert!(parse_at("gestern", start).is_err());
    }

    #[test]
    fn mention_parsing() {
        let st = crate::tests::state();
        let (text, params) = st.mentions("@mira schau mal, @niemand");
        assert_eq!(text, "{mention-user1} schau mal, @niemand");
        assert_eq!(params["mention-user1"]["id"], "mira");
    }
}
