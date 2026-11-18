//! Wire models for the Talk OCS API (spreed `api/v4/room`, `api/v1/chat`).
//!
//! Talk serialises "empty object" fields as `[]` in PHP style (e.g. `lastMessage`,
//! `messageParameters`, `reactions`), so those fields use lenient deserializers.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Conversation type, see spreed `docs/constants.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum RoomType {
    OneToOne,
    Group,
    Public,
    Changelog,
    FormerOneToOne,
    NoteToSelf,
    Unknown(u8),
}

impl From<u8> for RoomType {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::OneToOne,
            2 => Self::Group,
            3 => Self::Public,
            4 => Self::Changelog,
            5 => Self::FormerOneToOne,
            6 => Self::NoteToSelf,
            other => Self::Unknown(other),
        }
    }
}

impl From<RoomType> for u8 {
    fn from(t: RoomType) -> u8 {
        match t {
            RoomType::OneToOne => 1,
            RoomType::Group => 2,
            RoomType::Public => 3,
            RoomType::Changelog => 4,
            RoomType::FormerOneToOne => 5,
            RoomType::NoteToSelf => 6,
            RoomType::Unknown(v) => v,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: i64,
    pub token: String,
    #[serde(rename = "type")]
    pub room_type: RoomType,
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub unread_messages: i64,
    #[serde(default)]
    pub unread_mention: bool,
    #[serde(default)]
    pub unread_mention_direct: bool,
    #[serde(default)]
    pub last_activity: i64,
    #[serde(default)]
    pub last_read_message: i64,
    #[serde(default)]
    pub last_common_read_message: i64,
    #[serde(default, deserialize_with = "object_or_empty_array")]
    pub last_message: Option<ChatMessage>,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub read_only: u8,
    /// Effective permissions of the current user (bit field, see Talk `constants.md`).
    /// 0 when the server does not send them (old cache entries).
    #[serde(default)]
    pub permissions: i64,
    #[serde(default)]
    pub has_call: bool,
    #[serde(default)]
    pub call_flag: u32,
    #[serde(default)]
    pub participant_type: u8,
    #[serde(default)]
    pub notification_level: u8,
    #[serde(default)]
    pub avatar_version: String,
    /// A picture was uploaded (not the generated one).
    #[serde(default)]
    pub is_custom_avatar: bool,
    /// `false` when we are the last moderator of a group (promote somebody first).
    #[serde(default = "yes")]
    pub can_leave_conversation: bool,
    #[serde(default)]
    pub can_delete_conversation: bool,
    /// Our own categories this conversation is in (`conversation-tags`, docs/categories.md).
    #[serde(default, deserialize_with = "id_list")]
    pub tag_ids: Vec<String>,
    /// Status of the other user in 1:1 conversations (only with `includeStatus=true`).
    #[serde(default)]
    pub status: Option<String>,
    /// Marked as important: notifies and rings even while the user's status is "do not
    /// disturb" (docs/notifications.md).
    #[serde(default)]
    pub is_important: bool,
    /// Marked as sensitive: the server shows no message preview for it.
    #[serde(default)]
    pub is_sensitive: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub token: String,
    pub actor_type: String,
    pub actor_id: String,
    pub actor_display_name: String,
    pub timestamp: i64,
    pub message: String,
    #[serde(default, deserialize_with = "map_or_empty_array")]
    pub message_parameters: BTreeMap<String, Value>,
    #[serde(default)]
    pub system_message: String,
    #[serde(default)]
    pub message_type: String,
    #[serde(default)]
    pub is_replyable: bool,
    #[serde(default)]
    pub markdown: bool,
    #[serde(default)]
    pub reference_id: String,
    #[serde(default)]
    pub expiration_timestamp: i64,
    #[serde(default, deserialize_with = "reaction_map")]
    pub reactions: BTreeMap<String, i64>,
    #[serde(default)]
    pub reactions_self: Vec<String>,
    #[serde(default)]
    pub last_edit_timestamp: Option<i64>,
    /// Sent without notifying anybody.
    #[serde(default)]
    pub silent: bool,
    /// The message this one replies to (only one level deep).
    #[serde(default, deserialize_with = "parent_message")]
    pub parent: Option<Box<ChatMessage>>,
}

impl ChatMessage {
    pub fn is_system(&self) -> bool {
        !self.system_message.is_empty()
    }

    pub fn is_deleted(&self) -> bool {
        self.message_type == "comment_deleted"
    }

    /// The text as the user wrote it, to edit it: like [`Self::caption_text`], with
    /// mentions in the server's notation (`@"id"`). Their display
    /// names would come back as plain text and the mention would be gone.
    pub fn source_text(&self) -> String {
        if self.attachment().is_some() && self.message.trim() == "{file}" {
            return String::new();
        }
        let mut out = self.message.clone();
        for (key, param) in &self.message_parameters {
            let s = |k: &str| param.get(k).and_then(Value::as_str).unwrap_or_default();
            let replacement = match s("type") {
                "file" if key == "file" => String::new(),
                "user" | "guest" | "call" | "user-group" | "group" | "circle" | "team" | "email" => {
                    // Always quoted: a bare id would swallow a full stop that follows it.
                    let id = if s("mention-id").is_empty() { s("id") } else { s("mention-id") };
                    format!("@\"{id}\"")
                }
                _ => s("name").to_string(),
            };
            out = out.replace(&format!("{{{key}}}"), &replacement);
        }
        out
    }

    /// The shared file of a file message (`{file}` parameter of type `file`).
    pub fn attachment(&self) -> Option<FileAttachment> {
        let p = self.message_parameters.get("file")?;
        if p.get("type").and_then(Value::as_str) != Some("file") {
            return None;
        }
        let s = |k: &str| match p.get(k) {
            Some(Value::String(v)) => v.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let n = |k: &str| s(k).parse::<i64>().unwrap_or(0);
        let contact_name = match crate::contact::is_vcard(&s("mimetype")) {
            true => crate::contact::display_name(&s("contact-name"), &s("name")),
            false => String::new(),
        };
        Some(FileAttachment {
            id: s("id"),
            name: s("name"),
            path: s("path"),
            mimetype: s("mimetype"),
            contact_name,
            size: n("size"),
            width: n("width"),
            height: n("height"),
            preview_available: s("preview-available") == "yes",
            link: s("link"),
            voice_message: self.message_type == "voice-message",
        })
    }

    /// The place of a location message (`{object}` parameter of type `geo-location`).
    /// The coordinates come from `latitude`/`longitude`, else from the `geo:` id.
    pub fn location(&self) -> Option<GeoLocation> {
        let p = self.message_parameters.get("object")?;
        if p.get("type").and_then(Value::as_str) != Some("geo-location") {
            return None;
        }
        let num = |k: &str| match p.get(k) {
            Some(Value::String(v)) => v.trim().parse::<f64>().ok(),
            Some(Value::Number(n)) => n.as_f64(),
            _ => None,
        };
        let (latitude, longitude) = match (num("latitude"), num("longitude")) {
            (Some(lat), Some(lon)) => (lat, lon),
            _ => parse_geo_uri(p.get("id").and_then(Value::as_str)?)?,
        };
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return None;
        }
        let name = p.get("name").and_then(Value::as_str).unwrap_or_default().trim().to_string();
        Some(GeoLocation { latitude, longitude, name })
    }

    /// Text a file message carries besides the file: its caption (empty for plain `{file}`).
    /// For other messages the same as [`Self::rendered_text`].
    pub fn caption_text(&self) -> String {
        if self.attachment().is_some() {
            if self.message.trim() == "{file}" {
                return String::new();
            }
            return self.rendered_text_without("file");
        }
        self.rendered_text()
    }

    /// Message text with `{placeholders}` replaced by their display names.
    pub fn rendered_text(&self) -> String {
        self.rendered_text_without("")
    }

    fn rendered_text_without(&self, skip: &str) -> String {
        let mut out = self.message.clone();
        for (key, param) in &self.message_parameters {
            if key == skip {
                out = out.replace(&format!("{{{key}}}"), "");
                continue;
            }
            let name = param
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let replacement = match param.get("type").and_then(Value::as_str) {
                Some("user" | "guest" | "call" | "user-group" | "group") => format!("@{name}"),
                _ => name.to_string(),
            };
            out = out.replace(&format!("{{{key}}}"), &replacement);
        }
        out
    }
}

/// A file shared into a conversation. `width`/`height` are 0 when the server has no
/// image metadata; `id` is the file id for previews.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachment {
    pub id: String,
    pub name: String,
    pub path: String,
    pub mimetype: String,
    pub size: i64,
    pub width: i64,
    pub height: i64,
    pub preview_available: bool,
    pub link: String,
    /// Sent as a voice message (`messageType: voice-message`).
    pub voice_message: bool,
    /// The person of a shared contact (a vCard file), never empty for one; empty for
    /// every other file. Who has it draws the contact tile instead of the file tile.
    #[serde(default)]
    pub contact_name: String,
}

impl FileAttachment {
    pub fn is_image(&self) -> bool {
        self.mimetype.starts_with("image/")
    }
}

/// A place shared into a conversation (WGS 84, degrees). `name` is what the sender called
/// it, usually an address; it may be empty.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub name: String,
}

impl GeoLocation {
    /// `geo:<lat>,<lon>` with six decimals (about 10 cm), the object id Talk expects.
    pub fn geo_uri(&self) -> String {
        format!("geo:{:.6},{:.6}", self.latitude, self.longitude)
    }
}

/// Latitude and longitude of a `geo:` URI (RFC 5870); altitude and parameters are ignored.
fn parse_geo_uri(uri: &str) -> Option<(f64, f64)> {
    let rest = uri.get(..4).filter(|p| p.eq_ignore_ascii_case("geo:")).map(|_| &uri[4..])?;
    let mut parts = rest.split(';').next()?.split(',');
    let lat = parts.next()?.trim().parse::<f64>().ok()?;
    let lon = parts.next()?.trim().parse::<f64>().ok()?;
    Some((lat, lon))
}

/// Result of `GET /ocs/v2.php/apps/spreed/api/v3/signaling/settings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalingSettings {
    pub signaling_mode: String,
    pub user_id: Option<String>,
    /// Single URL or list of URLs of the external signaling server.
    #[serde(default)]
    pub server: Value,
    #[serde(default)]
    pub ticket: String,
    #[serde(default)]
    pub hello_auth_params: Value,
    #[serde(default)]
    pub stunservers: Vec<IceServer>,
    #[serde(default)]
    pub turnservers: Vec<IceServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceServer {
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub credential: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCredentials {
    pub server: String,
    pub login_name: String,
    pub app_password: String,
}

/// One reaction as listed by `GET /reaction/{token}/{messageId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    #[serde(default)]
    pub actor_type: String,
    #[serde(default)]
    pub actor_id: String,
    #[serde(default)]
    pub actor_display_name: String,
    #[serde(default)]
    pub timestamp: i64,
}

/// All reactions of a message: emoji → who reacted (the reaction endpoints' `data`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct Reactions(#[serde(deserialize_with = "reaction_map")] pub BTreeMap<String, Vec<Reaction>>);

/// One member of a conversation (`GET /room/{token}/participants`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub attendee_id: i64,
    #[serde(default)]
    pub actor_type: String,
    #[serde(default)]
    pub actor_id: String,
    #[serde(default)]
    pub display_name: String,
    /// 1 owner, 2 moderator, 3 user, 4 guest, 5 self-joined user, 6 guest moderator.
    #[serde(default)]
    pub participant_type: u8,
}

/// One of our categories (`GET /tags`, docs/categories.md). The server keeps one each of
/// type `favorites` and `other` ("Chats") besides our own (`custom`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTag {
    #[serde(deserialize_with = "id_string")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default, rename = "type")]
    pub kind: String,
}

fn yes() -> bool {
    true
}

/// `null` as empty string.
pub(crate) fn string_or_null<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// An id the server may send as number or string.
fn id_string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        v => Err(serde::de::Error::custom(format!("id: {v}"))),
    }
}

/// A list of ids (strings or numbers); `null` or anything else as empty.
fn id_list<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(a) => a
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

/// Talk returns `[]` instead of `null`/`{}` for empty objects.
fn object_or_empty_array<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Array(a) if a.is_empty() => Ok(None),
        v => serde_json::from_value(v)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// The quoted message of a reply. For one the user may no longer see the server sends
/// only `{"id": …, "deleted": true}`: that is a deleted message, not a broken answer.
fn parent_message<'de, D>(d: D) -> Result<Option<Box<ChatMessage>>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = match Value::deserialize(d)? {
        Value::Null => return Ok(None),
        Value::Array(a) if a.is_empty() => return Ok(None),
        v => v,
    };
    let stub = v.get("actorType").is_none();
    if stub {
        let Some(id) = v.get("id").and_then(Value::as_i64) else { return Ok(None) };
        return Ok(Some(Box::new(ChatMessage {
            id,
            message_type: "comment_deleted".into(),
            ..Default::default()
        })));
    }
    serde_json::from_value(v).map(Some).map_err(serde::de::Error::custom)
}

/// Like [`map_or_empty_array`], without keys that are no emoji: right after a reaction is
/// removed the server may briefly list the deleted comment, whose text is a JSON object
/// (`{"deleted_by_type":…}`).
fn reaction_map<'de, D, T>(d: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let mut map: BTreeMap<String, T> = map_or_empty_array(d)?;
    map.retain(|k, _| !k.is_empty() && !k.starts_with('{'));
    Ok(map)
}

fn map_or_empty_array<'de, D, T>(d: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    match Value::deserialize(d)? {
        Value::Null => Ok(BTreeMap::new()),
        Value::Array(a) if a.is_empty() => Ok(BTreeMap::new()),
        v => serde_json::from_value(v).map_err(serde::de::Error::custom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleted_reaction_comments_are_dropped() {
        let r: Reactions = serde_json::from_str(
            r#"{"👍":[{"actorType":"users","actorId":"b"}],
                "{\"deleted_by_type\":\"users\",\"deleted_by_id\":\"a\",\"deleted_on\":1}":[{"actorType":"users","actorId":"a"}]}"#,
        )
        .unwrap();
        assert_eq!(r.0.keys().collect::<Vec<_>>(), vec!["👍"]);
    }

    #[test]
    fn room_with_empty_last_message() {
        let json =
            r#"{"id":1,"token":"abc","type":2,"name":"n","displayName":"N","lastMessage":[]}"#;
        let room: Room = serde_json::from_str(json).unwrap();
        assert!(room.last_message.is_none());
        assert_eq!(room.room_type, RoomType::Group);
    }

    #[test]
    fn message_renders_mentions() {
        let json = r#"{"id":5,"token":"abc","actorType":"users","actorId":"a","actorDisplayName":"A",
            "timestamp":1,"message":"Hi {mention-user1}","messageParameters":
            {"mention-user1":{"type":"user","id":"b","name":"Bea"}},"reactions":[]}"#;
        let msg: ChatMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.rendered_text(), "Hi @Bea");
        assert!(msg.reactions.is_empty());
    }

    /// What goes into the input field to edit a message: mentions as the server reads
    /// them, the caption of a file without the file.
    #[test]
    fn source_text_keeps_mentions_and_drops_the_file() {
        let msg = |message: &str, params: &str| -> ChatMessage {
            let json = format!(
                r#"{{"id":5,"token":"abc","actorType":"users","actorId":"a","actorDisplayName":"A","timestamp":1,
                    "message":"{message}","messageParameters":{params}}}"#
            );
            serde_json::from_str(&json).unwrap()
        };
        let mentions = r#"{"mention-user1":{"type":"user","id":"b","name":"Bea","mention-id":"b"},
            "mention-call1":{"type":"call","id":"abc","name":"Garten","mention-id":"all"},
            "mention-group1":{"type":"user-group","id":"team eins","name":"Team","mention-id":"group/team eins"},
            "mention-guest1":{"type":"guest","id":"guest/1f","name":"Gast"}}"#;
        let m = msg("{mention-user1}. {mention-call1} {mention-group1} {mention-guest1}", mentions);
        assert_eq!(m.source_text(), r#"@"b". @"all" @"group/team eins" @"guest/1f""#);
        assert_eq!(m.rendered_text(), "@Bea. @Garten @Team @Gast");
        let file = r#"{"file":{"type":"file","id":"7","name":"Beet.jpg","path":"Talk/Beet.jpg","mimetype":"image/jpeg"}}"#;
        assert_eq!(msg("{file}", file).source_text(), "");
        assert_eq!(msg("Unser Beet", file).source_text(), "Unser Beet");
        assert_eq!(msg("Nur Text", "[]").source_text(), "Nur Text");
    }

    /// A quote the user may not see is only `{id, deleted}`: a deleted message, and the
    /// rest of the answer still parses.
    #[test]
    fn a_quote_reduced_to_its_id_is_a_deleted_message() {
        let json = r#"[{"id":6,"token":"abc","actorType":"users","actorId":"a","actorDisplayName":"A","timestamp":2,
            "message":"Antwort","parent":{"id":5,"deleted":true}}]"#;
        let list: Vec<ChatMessage> = serde_json::from_str(json).unwrap();
        let quote = list[0].parent.as_ref().unwrap();
        assert!(quote.id == 5 && quote.is_deleted());
        // It also survives the cache (stored as JSON).
        let again: ChatMessage = serde_json::from_str(&serde_json::to_string(&list[0]).unwrap()).unwrap();
        assert!(again.parent.unwrap().is_deleted());
    }

    #[test]
    fn file_message_with_caption() {
        let json = r#"{"id":6,"token":"abc","actorType":"users","actorId":"a","actorDisplayName":"A",
            "timestamp":1,"message":"Look at this","messageType":"comment","messageParameters":
            {"actor":{"type":"user","id":"a","name":"A"},"file":{"type":"file","id":"55341","name":"IMG.jpeg",
            "size":"1709918","path":"Talk/IMG.jpeg","mimetype":"image/jpeg","preview-available":"yes"}}}"#;
        let msg: ChatMessage = serde_json::from_str(json).unwrap();
        let a = msg.attachment().unwrap();
        assert_eq!(a.id, "55341");
        assert_eq!(a.size, 1709918);
        assert!(a.is_image() && a.preview_available);
        assert_eq!(msg.caption_text(), "Look at this");
    }

    #[test]
    fn file_message_without_caption() {
        let json = r#"{"id":7,"token":"abc","actorType":"users","actorId":"a","actorDisplayName":"A",
            "timestamp":1,"message":"{file}","messageParameters":
            {"file":{"type":"file","id":"1","name":"a.pdf","size":3,"mimetype":"application/pdf"}}}"#;
        let msg: ChatMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.caption_text(), "");
        assert_eq!(msg.attachment().unwrap().size, 3);
    }

    fn object_message(object: &str) -> ChatMessage {
        let json = format!(
            r#"{{"id":8,"token":"abc","actorType":"users","actorId":"a","actorDisplayName":"A",
            "timestamp":1,"message":"{{object}}","messageType":"comment","messageParameters":
            {{"actor":{{"type":"user","id":"a","name":"A"}},"object":{object}}}}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn location_message() {
        let msg = object_message(
            r#"{"type":"geo-location","id":"geo:52.5450511,13.3741463","name":"Büro, Berlin",
            "latitude":"52.5450511","longitude":"13.3741463"}"#,
        );
        let l = msg.location().unwrap();
        assert_eq!((l.latitude, l.longitude), (52.5450511, 13.3741463));
        assert_eq!(l.name, "Büro, Berlin");
        // The text stays the name, for everything that does not know locations.
        assert_eq!(msg.caption_text(), "Büro, Berlin");
        assert!(msg.attachment().is_none());
        assert_eq!(l.geo_uri(), "geo:52.545051,13.374146");
    }

    #[test]
    fn location_from_the_geo_id_alone() {
        let l = object_message(r#"{"type":"geo-location","id":"GEO:-33.9,151.2,12;crs=wgs84;u=35","name":"Hafen"}"#)
            .location()
            .unwrap();
        assert_eq!((l.latitude, l.longitude), (-33.9, 151.2));
    }

    #[test]
    fn other_objects_and_bad_coordinates_are_no_location() {
        assert!(object_message(r#"{"type":"talk-poll","id":"3","name":"Wann?"}"#).location().is_none());
        assert!(object_message(r#"{"type":"geo-location","id":"geo:91,10","name":"x"}"#).location().is_none());
        assert!(object_message(r#"{"type":"geo-location","id":"somewhere","name":"x"}"#).location().is_none());
    }
}
