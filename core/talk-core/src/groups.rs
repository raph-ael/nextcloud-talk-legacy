//! Groups like Signal: starting chats, creating groups, members and group settings.
//! The contract for the platforms is docs/groups.md.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageReader};
use serde::Serialize;

use crate::engine::{Engine, Event};
use crate::error::{Error, Result};
use crate::models::{Participant, Room};

/// Edge length of uploaded group pictures.
const PHOTO_SIZE: u32 = 512;
const PHOTO_QUALITY: u8 = 85;
/// Users listed without a search term ("Kontakte").
const CONTACTS_LIMIT: u32 = 50;
const SEARCH_LIMIT: u32 = 30;

/// One member of a group, for the member list in the group settings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    /// Pass to `remove_member` / `set_moderator`.
    pub attendee_id: i64,
    /// `users`, `groups`, `guests`, `emails`, `federated_users` …
    pub actor_type: String,
    /// The user id for `users` (avatar, `open_direct`).
    pub actor_id: String,
    pub name: String,
    pub is_self: bool,
    /// Owner or moderator.
    pub is_moderator: bool,
    /// The owner's rights cannot be changed and they cannot be removed.
    pub is_owner: bool,
}

/// A user from the search ("Neue Nachricht", "Mitglieder hinzufügen").
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    pub id: String,
    pub name: String,
    /// Extra line from the server (e.g. e-mail or status), may be empty.
    pub subline: String,
}

/// Which action failed, for [`group_error_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupOp {
    Search,
    OpenDirect,
    Create,
    Members,
    AddMembers,
    RemoveMember,
    SetModerator,
    Rename,
    Describe,
    Photo,
    Link,
    Mute,
    Leave,
    Delete,
}

impl Engine {
    /// Whether the server lets this user create groups (hide "Neue Gruppe" otherwise).
    pub fn can_create_groups(&self) -> bool {
        self.can_create.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Users to start a chat with or to add: everybody the server lists for an empty
    /// `query` (alphabetical), else the matches in the server's order. Never ourselves;
    /// with `exclude_token` not the members of that conversation.
    pub async fn search_users(&self, query: &str, exclude_token: Option<&str>) -> Result<Vec<UserView>> {
        let query = query.trim();
        let limit = if query.is_empty() { CONTACTS_LIMIT } else { SEARCH_LIMIT };
        let (hits, members) = tokio::join!(self.client.search_users(query, limit), async {
            match exclude_token {
                Some(t) => self.client.members(t).await.map(Some),
                None => Ok(None),
            }
        });
        let hits = self.guard(hits)?;
        let members = self.guard(members)?.unwrap_or_default();
        let me = self.user_id();
        let mut users: Vec<UserView> = hits
            .into_iter()
            .filter(|h| h.source.is_empty() || h.source == "users")
            .filter(|h| h.id != me)
            .filter(|h| !members.iter().any(|m| m.actor_type == "users" && m.actor_id == h.id))
            .map(|h| UserView {
                name: if h.label.is_empty() { h.id.clone() } else { h.label },
                id: h.id,
                subline: h.subline,
            })
            .collect();
        if query.is_empty() {
            users.sort_by_key(|u| u.name.to_lowercase());
        }
        Ok(users)
    }

    /// The one-to-one chat with `user_id` (created when there is none). Returns its token;
    /// the conversation is in the list right away.
    pub async fn open_direct(&self, user_id: &str) -> Result<String> {
        let room = self.guard(self.client.create_room(1, "", user_id, &[]).await)?;
        let token = room.token.clone();
        self.upsert_room(room);
        Ok(token)
    }

    /// Creates a group with `user_ids` as members and an optional picture (any image the
    /// platform picked; it is cropped to a square here). Returns the token; the group is
    /// in the list right away. A picture that cannot be used does not fail the creation.
    pub async fn create_group(&self, name: &str, user_ids: &[String], photo: Option<Vec<u8>>) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Http { status: 400, message: "empty name".into() });
        }
        let room = self.guard(self.client.create_room(2, name, "", user_ids).await)?;
        let token = room.token.clone();
        self.upsert_room(room);
        if !user_ids.is_empty() {
            // Servers without `participants` on creation: add who is missing.
            let members = self.guard(self.client.members(&token).await).unwrap_or_default();
            for u in user_ids {
                if !members.iter().any(|m| m.actor_type == "users" && &m.actor_id == u) {
                    if let Err(e) = self.guard(self.client.add_participant(&token, u).await) {
                        tracing::warn!(%token, "adding {u} to the new group failed: {e}");
                    }
                }
            }
        }
        if let Some(bytes) = photo {
            match square_photo_async(bytes).await {
                Ok(jpeg) => {
                    if let Err(e) = self.guard(self.client.upload_room_avatar(&token, jpeg).await) {
                        tracing::warn!(%token, "group picture upload failed: {e}");
                    }
                }
                Err(e) => tracing::warn!(%token, "group picture unusable: {e}"),
            }
        }
        let _ = self.refresh_rooms(true).await;
        Ok(token)
    }

    /// Members of a conversation like Signal: ourselves first, then the moderators, then
    /// everybody else alphabetically.
    pub async fn members(&self, token: &str) -> Result<Vec<MemberView>> {
        let list = self.guard(self.client.members(token).await)?;
        Ok(member_views(list, |t, id| self.is_own(t, id)))
    }

    /// Adds users to a group (moderators). Tries all of them; returns the first error.
    pub async fn add_members(&self, token: &str, user_ids: &[String]) -> Result<()> {
        let mut first_err = None;
        for u in user_ids {
            if let Err(e) = self.guard(self.client.add_participant(token, u).await) {
                first_err.get_or_insert(e);
            }
        }
        self.members_changed(token).await;
        first_err.map_or(Ok(()), Err)
    }

    pub async fn remove_member(&self, token: &str, attendee_id: i64) -> Result<()> {
        let r = self.guard(self.client.remove_attendee(token, attendee_id).await);
        self.members_changed(token).await;
        r
    }

    /// Makes a member moderator (`true`) or takes the rights away.
    pub async fn set_moderator(&self, token: &str, attendee_id: i64, moderator: bool) -> Result<()> {
        let r = self.guard(self.client.set_moderator(token, attendee_id, moderator).await);
        self.members_changed(token).await;
        r
    }

    pub async fn rename_group(&self, token: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Http { status: 400, message: "empty name".into() });
        }
        let r = self.guard(self.client.rename_room(token, name).await);
        self.room_changed().await;
        r
    }

    pub async fn set_description(&self, token: &str, description: &str) -> Result<()> {
        let r = self.guard(self.client.set_description(token, description.trim()).await);
        self.room_changed().await;
        r
    }

    /// Sets the group picture (any image, cropped to a square here) or removes it (`None`).
    pub async fn set_group_photo(&self, token: &str, photo: Option<Vec<u8>>) -> Result<()> {
        let r = match photo {
            Some(bytes) => {
                let jpeg = square_photo_async(bytes).await?;
                self.guard(self.client.upload_room_avatar(token, jpeg).await)
            }
            None => self.guard(self.client.delete_room_avatar(token).await),
        };
        self.room_changed().await;
        r
    }

    /// Group link on (anyone with [`Self::group_link`] can join) or off.
    pub async fn set_group_link(&self, token: &str, on: bool) -> Result<()> {
        let r = self.guard(self.client.set_public(token, on).await);
        self.room_changed().await;
        r
    }

    /// Link to join the group in a browser or the Talk apps.
    pub fn group_link(&self, token: &str) -> String {
        format!("{}/index.php/call/{token}", self.client.account().server)
    }

    /// 1 always, 2 mentions only, 3 never.
    pub async fn set_notification_level(&self, token: &str, level: u8) -> Result<()> {
        let r = self.guard(self.client.set_notification_level(token, level).await);
        self.room_changed().await;
        r
    }

    /// Leaves the group for good. Fails (HTTP 400) for its last moderator.
    pub async fn leave_group(&self, token: &str) -> Result<()> {
        self.guard(self.client.leave_conversation(token).await)?;
        self.room_gone(token).await;
        Ok(())
    }

    /// Deletes the group and all its messages for everybody (moderators).
    pub async fn delete_group(&self, token: &str) -> Result<()> {
        self.guard(self.client.delete_room(token).await)?;
        self.room_gone(token).await;
        Ok(())
    }

    async fn members_changed(&self, token: &str) {
        self.sink.emit(Event::ParticipantsChanged { token: token.to_string() });
        self.room_changed().await;
    }

    async fn room_changed(&self) {
        let _ = self.refresh_rooms(true).await;
    }

    async fn room_gone(&self, token: &str) {
        self.close_chat(token);
        let _ = self.refresh_rooms(true).await;
    }

    /// A conversation the server just returned: into the list at once.
    fn upsert_room(&self, room: Room) {
        self.put_room(room);
    }
}

/// Members sorted like Signal: ourselves first, then the moderators, then by name.
fn member_views(list: Vec<Participant>, is_own: impl Fn(&str, &str) -> bool) -> Vec<MemberView> {
    let mut out: Vec<MemberView> = list
        .into_iter()
        .map(|p| MemberView {
            is_self: is_own(&p.actor_type, &p.actor_id),
            is_moderator: matches!(p.participant_type, 1 | 2 | 6),
            is_owner: p.participant_type == 1,
            name: if p.display_name.is_empty() {
                if p.actor_type == "guests" { "Gast".into() } else { p.actor_id.clone() }
            } else {
                p.display_name
            },
            attendee_id: p.attendee_id,
            actor_type: p.actor_type,
            actor_id: p.actor_id,
        })
        .collect();
    out.sort_by(|a, b| {
        b.is_self
            .cmp(&a.is_self)
            .then(b.is_moderator.cmp(&a.is_moderator))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// German text for a failed group action, for a short notice.
pub fn group_error_text(op: GroupOp, e: &Error) -> String {
    use GroupOp::*;
    let text = match (op, e) {
        (_, Error::Network(_) | Error::RetryLater { .. }) => "Keine Verbindung",
        (Photo | Create, Error::LocalFile(_)) => "Bild konnte nicht verwendet werden",
        (Leave, Error::Http { status: 400, .. }) => "Ernenne zuerst einen anderen Moderator",
        (Create | Rename, Error::Http { status: 400, .. }) => "Bitte einen Gruppennamen eingeben",
        (Describe, Error::Http { status: 400, .. }) => "Die Beschreibung ist zu lang",
        (Photo, Error::Http { status: 400, .. }) => "Bild konnte nicht verwendet werden",
        (RemoveMember, Error::Http { status: 400, .. }) => "Entziehe zuerst die Moderatorrechte",
        (SetModerator, Error::Http { status: 400, .. }) => "Die Rechte dieser Person lassen sich nicht ändern",
        (Create, Error::Http { status: 403, .. }) => "Du darfst keine Gruppen erstellen",
        (_, Error::Http { status: 403, .. }) => "Nur Moderatoren können das",
        (OpenDirect | AddMembers, Error::NotFound) => "Nutzer nicht gefunden",
        (_, Error::NotFound) => "Gruppe nicht gefunden",
        (Search, _) => "Suche fehlgeschlagen",
        (OpenDirect, _) => "Chat konnte nicht geöffnet werden",
        (Create, _) => "Gruppe konnte nicht erstellt werden",
        (Members, _) => "Mitglieder konnten nicht geladen werden",
        (AddMembers, _) => "Hinzufügen fehlgeschlagen",
        (RemoveMember, _) => "Entfernen fehlgeschlagen",
        (SetModerator, _) => "Rechte konnten nicht geändert werden",
        (Rename | Describe | Photo | Link | Mute, _) => "Speichern fehlgeschlagen",
        (Leave, _) => "Verlassen fehlgeschlagen",
        (Delete, _) => "Löschen fehlgeschlagen",
    };
    text.into()
}

async fn square_photo_async(bytes: Vec<u8>) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || square_photo(&bytes))
        .await
        .map_err(|e| Error::LocalFile(e.to_string()))?
}

/// Any picture (JPEG, PNG, WebP; camera rotation applied) as a centered square JPEG of
/// at most [`PHOTO_SIZE`] px, as Talk wants it for conversation pictures.
pub fn square_photo(bytes: &[u8]) -> Result<Vec<u8>> {
    let bad = |e: image::ImageError| Error::LocalFile(format!("Bild: {e}"));
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Error::LocalFile(format!("Bild: {e}")))?
        .into_decoder()
        .map_err(bad)?;
    let orientation = decoder.orientation().map_err(bad)?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(bad)?;
    img.apply_orientation(orientation);
    let side = img.width().min(img.height());
    if side == 0 {
        return Err(Error::LocalFile("Bild ist leer".into()));
    }
    let x = (img.width() - side) / 2;
    let y = (img.height() - side) / 2;
    let mut square = img.crop_imm(x, y, side, side);
    if side > PHOTO_SIZE {
        square = square.resize_exact(PHOTO_SIZE, PHOTO_SIZE, image::imageops::FilterType::Lanczos3);
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, PHOTO_QUALITY)
        .encode_image(&square.to_rgb8())
        .map_err(bad)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_becomes_centered_square_jpeg() {
        // 900×600, left third red, middle green, right third blue.
        let img = image::RgbImage::from_fn(900, 600, |x, _| match x / 300 {
            0 => image::Rgb([255, 0, 0]),
            1 => image::Rgb([0, 255, 0]),
            _ => image::Rgb([0, 0, 255]),
        });
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let jpeg = square_photo(&png).unwrap();
        let out = image::load_from_memory(&jpeg).unwrap().to_rgb8();
        assert_eq!((out.width(), out.height()), (512, 512));
        // Cropped from x=150..750: the middle is green, the edges red and blue.
        assert!(out.get_pixel(256, 256)[1] > 200);
        assert!(out.get_pixel(5, 256)[0] > 200);
        assert!(out.get_pixel(506, 256)[2] > 200);
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn small_photo_is_not_enlarged_and_garbage_fails() {
        let img = image::RgbImage::from_pixel(100, 300, image::Rgb([9, 9, 9]));
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let out = image::load_from_memory(&square_photo(&png).unwrap()).unwrap();
        assert_eq!((out.width(), out.height()), (100, 100));
        assert!(matches!(square_photo(b"no image"), Err(Error::LocalFile(_))));
    }

    #[test]
    fn participants_parse() {
        let list: Vec<Participant> = serde_json::from_str(
            r#"[{"attendeeId":3,"actorType":"users","actorId":"a","displayName":"Anna","participantType":1,"inCall":0},
                {"attendeeId":4,"actorType":"guests","actorId":"x","displayName":"","participantType":4}]"#,
        )
        .unwrap();
        assert_eq!(list[0].attendee_id, 3);
        assert_eq!(list[0].participant_type, 1);
        assert_eq!(list[1].actor_type, "guests");
    }

    #[test]
    fn members_sorted_like_signal() {
        let list: Vec<Participant> = serde_json::from_str(
            r#"[{"attendeeId":1,"actorType":"users","actorId":"z","displayName":"Zoe","participantType":3},
                {"attendeeId":2,"actorType":"users","actorId":"b","displayName":"bert","participantType":2},
                {"attendeeId":3,"actorType":"users","actorId":"me","displayName":"Ich","participantType":3},
                {"attendeeId":4,"actorType":"users","actorId":"a","displayName":"Anna","participantType":3},
                {"attendeeId":5,"actorType":"users","actorId":"o","displayName":"Otto","participantType":1}]"#,
        )
        .unwrap();
        let v = member_views(list, |t, id| t == "users" && id == "me");
        let names: Vec<&str> = v.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Ich", "bert", "Otto", "Anna", "Zoe"]);
        assert!(v[0].is_self && !v[0].is_moderator);
        assert!(v[2].is_owner && v[2].is_moderator);
    }

    #[test]
    fn errors_are_readable() {
        let bad = Error::Http { status: 400, message: String::new() };
        let forbidden = Error::Http { status: 403, message: String::new() };
        assert_eq!(group_error_text(GroupOp::Leave, &bad), "Ernenne zuerst einen anderen Moderator");
        assert_eq!(group_error_text(GroupOp::RemoveMember, &forbidden), "Nur Moderatoren können das");
        assert_eq!(group_error_text(GroupOp::Create, &forbidden), "Du darfst keine Gruppen erstellen");
        assert_eq!(group_error_text(GroupOp::Delete, &Error::Decode("x".into())), "Löschen fehlgeschlagen");
        assert_eq!(group_error_text(GroupOp::Photo, &Error::LocalFile("x".into())), "Bild konnte nicht verwendet werden");
    }
}
