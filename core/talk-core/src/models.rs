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
    /// Status of the other user in 1:1 conversations (only with `includeStatus=true`).
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default, deserialize_with = "map_or_empty_array")]
    pub reactions: BTreeMap<String, i64>,
    #[serde(default)]
    pub reactions_self: Vec<String>,
    #[serde(default)]
    pub last_edit_timestamp: Option<i64>,
    /// The message this one replies to (only one level deep).
    #[serde(default, deserialize_with = "object_or_empty_array")]
    pub parent: Option<Box<ChatMessage>>,
}

impl ChatMessage {
    pub fn is_system(&self) -> bool {
        !self.system_message.is_empty()
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
        Some(FileAttachment {
            id: s("id"),
            name: s("name"),
            path: s("path"),
            mimetype: s("mimetype"),
            size: n("size"),
            width: n("width"),
            height: n("height"),
            preview_available: s("preview-available") == "yes",
            link: s("link"),
            voice_message: self.message_type == "voice-message",
        })
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
}

impl FileAttachment {
    pub fn is_image(&self) -> bool {
        self.mimetype.starts_with("image/")
    }
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
}
