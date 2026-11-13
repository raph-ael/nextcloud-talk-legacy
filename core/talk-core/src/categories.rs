//! Categories in the chat list: "Favoriten", our own categories, "Chats", each one
//! collapsible (Talk calls them conversation tags). The contract for the platforms is
//! docs/categories.md.

use std::collections::HashSet;

use serde::Serialize;

use crate::cache::Cache;
use crate::engine::{Engine, RoomView};
use crate::error::{Error, Result};
use crate::models::ConversationTag;

/// Section ids without server categories (old server, never loaded).
pub const LOCAL_FAVORITES: &str = "fav";
pub const LOCAL_OTHER: &str = "other";

const KIND_FAVORITES: &str = "favorites";
const KIND_OTHER: &str = "other";
const KIND_CUSTOM: &str = "custom";

/// What the engine knows about categories.
#[derive(Debug, Default)]
pub(crate) struct Categories {
    /// The server has `conversation-tags`.
    supported: bool,
    /// From the server (cached), any order.
    tags: Vec<ConversationTag>,
    /// Collapsed sections without server categories (ids [`LOCAL_FAVORITES`], [`LOCAL_OTHER`]).
    local_collapsed: HashSet<String>,
}

impl Categories {
    pub(crate) fn load(cache: &Cache) -> Self {
        Self {
            supported: cache.meta("tags_supported").is_some_and(|v| v == "true"),
            tags: cache
                .meta("tags")
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default(),
            local_collapsed: cache
                .meta("collapsed_local")
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default(),
        }
    }

    fn sorted(&self) -> Vec<ConversationTag> {
        let mut tags = self.tags.clone();
        tags.sort_by_key(|t| t.sort_order);
        tags
    }
}

/// One section of the chat list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionView {
    /// Pass to `toggle_section`; also part of the row keys (a chat may be in several).
    pub id: String,
    /// `favorites`, `custom` or `other`.
    pub kind: String,
    /// "Favoriten", the category name or "Chats". Empty when it is the only section:
    /// then show no header (and it is never collapsed).
    pub title: String,
    pub collapsed: bool,
    /// What to show: all chats, or when collapsed only the open one, unread ones and
    /// ones with a call.
    pub rooms: Vec<RoomView>,
    /// All chats in the section (also the hidden ones).
    pub total: u32,
}

/// One category for the desktop page "Kategorien".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryView {
    pub id: String,
    /// `favorites`, `custom` or `other`; only `custom` can be renamed, deleted and filled.
    pub kind: String,
    pub name: String,
    /// Chats in it (not archived).
    pub room_count: u32,
    pub collapsed: bool,
}

/// Which action failed, for [`category_error_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryOp {
    Collapse,
    Create,
    Rename,
    Delete,
    Move,
    Assign,
}

impl Engine {
    /// The server keeps categories (`conversation-tags`): show "Kategorien verwalten".
    pub fn categories_supported(&self) -> bool {
        self.categories.lock().unwrap().supported
    }

    pub(crate) fn set_categories_supported(&self, supported: bool) {
        let mut c = self.categories.lock().unwrap();
        if c.supported != supported {
            c.supported = supported;
            self.cache.set_meta("tags_supported", supported.to_string());
        }
    }

    /// Categories fetched from the server; does not emit (the room list follows).
    pub(crate) fn apply_tags(&self, tags: Result<Vec<ConversationTag>>) {
        let Ok(tags) = tags else { return };
        let mut c = self.categories.lock().unwrap();
        if c.tags != tags {
            self.cache.set_meta("tags", serde_json::to_string(&tags).unwrap_or_default());
            c.tags = tags;
        }
    }

    /// The chat list as sections (docs/categories.md): "Favoriten", our categories,
    /// "Chats" in the server's order; empty sections are left out. `query` filters by
    /// name (then nothing is collapsed). `active_token` stays visible when collapsed.
    pub fn room_sections(&self, query: &str, active_token: Option<&str>) -> Vec<SectionView> {
        let rooms = self.rooms();
        let c = self.categories.lock().unwrap();
        build_sections(rooms, &c.sorted(), &c.local_collapsed, query, active_token)
    }

    /// Opens or closes a section. Saved on the server (all devices, Talk-Web); without
    /// server categories only here. Shows the change at once and undoes it on failure.
    pub async fn toggle_section(&self, id: &str) -> Result<()> {
        let collapsed = {
            let mut c = self.categories.lock().unwrap();
            match c.tags.iter_mut().find(|t| t.id == id) {
                Some(t) => {
                    t.collapsed = !t.collapsed;
                    Some(t.collapsed)
                }
                None => {
                    if !c.local_collapsed.remove(id) {
                        c.local_collapsed.insert(id.to_string());
                    }
                    let json = serde_json::to_string(&c.local_collapsed).unwrap_or_default();
                    self.cache.set_meta("collapsed_local", json);
                    None
                }
            }
        };
        self.emit_rooms();
        let Some(collapsed) = collapsed else { return Ok(()) };
        match self.guard(self.client.set_tag_collapsed(id, collapsed).await) {
            Ok(tag) => {
                self.replace_tag(tag);
                Ok(())
            }
            Err(e) => {
                if let Some(t) = self.categories.lock().unwrap().tags.iter_mut().find(|t| t.id == id) {
                    t.collapsed = !collapsed;
                }
                self.emit_rooms();
                Err(e)
            }
        }
    }

    /// All categories in order, for the page "Kategorien" (empty without server support).
    pub fn categories(&self) -> Vec<CategoryView> {
        let rooms: Vec<RoomView> = self.rooms().into_iter().filter(|r| !r.is_archived).collect();
        let tags = self.categories.lock().unwrap().sorted();
        let custom: HashSet<&str> = tags.iter().filter(|t| t.kind == KIND_CUSTOM).map(|t| t.id.as_str()).collect();
        tags.iter()
            .map(|t| {
                let count = rooms
                    .iter()
                    .filter(|r| match t.kind.as_str() {
                        KIND_FAVORITES => r.is_favorite,
                        KIND_OTHER => !r.is_favorite && !has_category(r, &custom),
                        _ => r.tag_ids.contains(&t.id),
                    })
                    .count();
                CategoryView {
                    id: t.id.clone(),
                    kind: t.kind.clone(),
                    name: title(t),
                    room_count: count as u32,
                    collapsed: t.collapsed,
                }
            })
            .collect()
    }

    /// Creates a category with these chats in it. Returns its id.
    pub async fn create_category(&self, name: &str, tokens: &[String]) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Http { status: 400, message: r#"{"error":"name"}"#.into() });
        }
        let tag = self.guard(self.client.create_tag(name).await)?;
        let id = tag.id.clone();
        self.reload_tags().await;
        if !tokens.is_empty() {
            self.set_category_rooms(&id, tokens).await?;
        } else {
            self.emit_rooms();
        }
        Ok(id)
    }

    pub async fn rename_category(&self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Http { status: 400, message: r#"{"error":"name"}"#.into() });
        }
        let tag = self.guard(self.client.rename_tag(id, name).await)?;
        self.replace_tag(tag);
        self.emit_rooms();
        Ok(())
    }

    /// Deletes one of our categories; its chats stay (under "Chats" unless in another).
    pub async fn delete_category(&self, id: &str) -> Result<()> {
        self.guard(self.client.delete_tag(id).await)?;
        self.categories.lock().unwrap().tags.retain(|t| t.id != id);
        // The server took the category off every conversation.
        let _ = self.refresh_rooms(true).await;
        Ok(())
    }

    /// Moves a category one place up (`-1`) or down (`1`), built-in ones included.
    pub async fn move_category(&self, id: &str, offset: i32) -> Result<()> {
        let mut ids: Vec<String> = self.categories.lock().unwrap().sorted().into_iter().map(|t| t.id).collect();
        let Some(from) = ids.iter().position(|t| t == id) else {
            return Err(Error::NotFound);
        };
        let to = from as i64 + offset.signum() as i64;
        if to < 0 || to >= ids.len() as i64 {
            return Ok(());
        }
        ids.swap(from, to as usize);
        let tags = self.guard(self.client.reorder_tags(&ids).await)?;
        self.apply_tags(Ok(tags));
        self.emit_rooms();
        Ok(())
    }

    /// Makes exactly `tokens` the chats of category `id` (others leave it). Only the
    /// conversations that change are sent.
    pub async fn set_category_rooms(&self, id: &str, tokens: &[String]) -> Result<()> {
        let changes = tag_changes(&self.rooms(), id, tokens);
        let mut updated = Vec::new();
        let mut failed = None;
        for (token, tag_ids) in changes {
            match self.guard(self.client.set_room_tags(&token, &tag_ids).await) {
                Ok(room) => updated.push(room),
                Err(e) => {
                    tracing::warn!(%token, "assigning categories failed: {e}");
                    failed.get_or_insert(e);
                }
            }
        }
        if updated.is_empty() {
            self.emit_rooms();
        } else {
            self.put_rooms(updated);
        }
        failed.map_or(Ok(()), Err)
    }

    async fn reload_tags(&self) {
        let tags = self.guard(self.client.tags().await);
        self.apply_tags(tags);
    }

    fn replace_tag(&self, tag: ConversationTag) {
        let mut c = self.categories.lock().unwrap();
        if let Some(t) = c.tags.iter_mut().find(|t| t.id == tag.id) {
            *t = tag;
        }
        let json = serde_json::to_string(&c.tags).unwrap_or_default();
        self.cache.set_meta("tags", json);
    }
}

fn title(t: &ConversationTag) -> String {
    match t.kind.as_str() {
        KIND_FAVORITES => "Favoriten".into(),
        KIND_OTHER => "Chats".into(),
        _ => t.name.clone(),
    }
}

/// In one of our (existing) categories.
fn has_category(r: &RoomView, custom: &HashSet<&str>) -> bool {
    r.tag_ids.iter().any(|id| custom.contains(id.as_str()))
}

/// The sections like Talk-Web: a favorite is under "Favoriten" (and its categories),
/// a chat in no category and no favorite under "Chats".
pub(crate) fn build_sections(
    rooms: Vec<RoomView>,
    sorted_tags: &[ConversationTag],
    local_collapsed: &HashSet<String>,
    query: &str,
    active_token: Option<&str>,
) -> Vec<SectionView> {
    let q = query.trim().to_lowercase();
    let searching = !q.is_empty();
    let rooms: Vec<RoomView> = rooms
        .into_iter()
        .filter(|r| !r.is_archived && (!searching || r.name.to_lowercase().contains(&q)))
        .collect();

    let local;
    let tags = if sorted_tags.iter().any(|t| t.kind == KIND_FAVORITES) && sorted_tags.iter().any(|t| t.kind == KIND_OTHER) {
        sorted_tags
    } else {
        let tag = |id: &str, kind: &str| ConversationTag {
            id: id.into(),
            name: String::new(),
            sort_order: 0,
            collapsed: local_collapsed.contains(id),
            kind: kind.into(),
        };
        local = [tag(LOCAL_FAVORITES, KIND_FAVORITES), tag(LOCAL_OTHER, KIND_OTHER)];
        &local[..]
    };
    let custom: HashSet<&str> = tags.iter().filter(|t| t.kind == KIND_CUSTOM).map(|t| t.id.as_str()).collect();

    let mut sections: Vec<SectionView> = tags
        .iter()
        .filter_map(|t| {
            let members: Vec<RoomView> = rooms
                .iter()
                .filter(|r| match t.kind.as_str() {
                    KIND_FAVORITES => r.is_favorite,
                    KIND_OTHER => !r.is_favorite && !has_category(r, &custom),
                    _ => r.tag_ids.contains(&t.id),
                })
                .cloned()
                .collect();
            (!members.is_empty()).then(|| SectionView {
                id: t.id.clone(),
                kind: t.kind.clone(),
                title: title(t),
                collapsed: t.collapsed && !searching,
                total: members.len() as u32,
                rooms: members,
            })
        })
        .collect();

    if let [only] = sections.as_mut_slice() {
        only.title.clear();
        only.collapsed = false;
    }
    for s in &mut sections {
        if s.collapsed {
            s.rooms
                .retain(|r| Some(r.token.as_str()) == active_token || r.unread_messages > 0 || r.has_call);
        }
    }
    sections
}

/// The conversations whose category list changes when `id` should hold exactly `tokens`,
/// with their new list.
fn tag_changes(rooms: &[RoomView], id: &str, tokens: &[String]) -> Vec<(String, Vec<String>)> {
    rooms
        .iter()
        .filter_map(|r| {
            let has = r.tag_ids.iter().any(|t| t == id);
            let want = tokens.contains(&r.token);
            if has == want {
                return None;
            }
            let mut ids: Vec<String> = r.tag_ids.iter().filter(|t| *t != id).cloned().collect();
            if want {
                ids.push(id.to_string());
            }
            Some((r.token.clone(), ids))
        })
        .collect()
}

/// German message for a failed category action.
pub fn category_error_text(op: CategoryOp, e: &Error) -> String {
    use CategoryOp::*;
    let text = match (op, e) {
        (_, Error::Network(_) | Error::RetryLater { .. }) => "Keine Verbindung",
        (Create, Error::Http { status: 400, message }) if message.contains("limit") => "Zu viele Kategorien",
        (Create | Rename, Error::Http { status: 400, .. }) => "Name ist leer oder schon vergeben",
        (_, Error::NotFound) => "Kategorie nicht gefunden",
        (Collapse, _) => "Konnte nicht gespeichert werden",
        (Create, _) => "Kategorie konnte nicht erstellt werden",
        (Rename | Assign, _) => "Speichern fehlgeschlagen",
        (Delete, _) => "Löschen fehlgeschlagen",
        (Move, _) => "Verschieben fehlgeschlagen",
    };
    text.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(token: &str, fav: bool, tags: &[&str]) -> RoomView {
        RoomView {
            token: token.into(),
            name: token.to_uppercase(),
            description: String::new(),
            room_type: 2,
            unread_messages: 0,
            unread_mention: false,
            last_activity: 0,
            last_read_message: 0,
            last_common_read_message: 0,
            last_message: None,
            is_favorite: fav,
            is_archived: false,
            read_only: false,
            can_react: true,
            has_call: false,
            notification_level: 0,
            avatar_version: String::new(),
            has_custom_avatar: false,
            is_moderator: false,
            can_leave: true,
            can_delete: false,
            is_public: false,
            tag_ids: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    fn tag(id: &str, kind: &str, name: &str, order: i64, collapsed: bool) -> ConversationTag {
        ConversationTag { id: id.into(), name: name.into(), sort_order: order, collapsed, kind: kind.into() }
    }

    fn server_tags() -> Vec<ConversationTag> {
        vec![
            tag("1", "favorites", "Favorites", 0, false),
            tag("3", "custom", "Arbeit", 1, false),
            tag("4", "custom", "Familie", 2, false),
            tag("2", "other", "Other", 3, false),
        ]
    }

    fn ids(s: &SectionView) -> Vec<&str> {
        s.rooms.iter().map(|r| r.token.as_str()).collect()
    }

    #[test]
    fn sections_like_talk_web() {
        let rooms = vec![
            room("a", true, &[]),
            room("b", false, &["3"]),
            room("c", false, &["3", "4"]),
            room("d", false, &[]),
            room("e", true, &["4"]),
            room("f", false, &["99"]), // unknown category: under "Chats"
        ];
        let s = build_sections(rooms, &server_tags(), &HashSet::new(), "", None);
        let titles: Vec<&str> = s.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Favoriten", "Arbeit", "Familie", "Chats"]);
        assert_eq!(ids(&s[0]), ["a", "e"]);
        assert_eq!(ids(&s[1]), ["b", "c"]);
        assert_eq!(ids(&s[2]), ["c", "e"]);
        assert_eq!(ids(&s[3]), ["d", "f"]);
        assert_eq!(s[1].kind, "custom");
    }

    #[test]
    fn empty_sections_and_single_section() {
        let s = build_sections(vec![room("a", false, &[])], &server_tags(), &HashSet::new(), "", None);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "");
        let mut tags = server_tags();
        tags[3].collapsed = true;
        let s = build_sections(vec![room("a", false, &[])], &tags, &HashSet::new(), "", None);
        assert!(!s[0].collapsed);
        assert_eq!(ids(&s[0]), ["a"]);
    }

    #[test]
    fn collapsed_keeps_active_unread_and_calls() {
        let mut tags = server_tags();
        tags[1].collapsed = true;
        let mut unread = room("u", false, &["3"]);
        unread.unread_messages = 2;
        let mut call = room("k", false, &["3"]);
        call.has_call = true;
        let rooms = vec![room("x", false, &["3"]), unread, call, room("act", false, &["3"]), room("d", false, &[])];
        let s = build_sections(rooms.clone(), &tags, &HashSet::new(), "", Some("act"));
        assert!(s[0].collapsed);
        assert_eq!(s[0].total, 4);
        assert_eq!(ids(&s[0]), ["u", "k", "act"]);
        // Searching opens everything.
        let s = build_sections(rooms, &tags, &HashSet::new(), "x", None);
        assert!(!s[0].collapsed);
        assert_eq!(ids(&s[0]), ["x"]);
    }

    #[test]
    fn local_sections_without_server() {
        let collapsed: HashSet<String> = [LOCAL_OTHER.to_string()].into();
        let mut archived = room("z", false, &[]);
        archived.is_archived = true;
        let rooms = vec![room("a", true, &[]), room("b", false, &["3"]), archived];
        let s = build_sections(rooms, &[], &collapsed, "", None);
        assert_eq!(s.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), [LOCAL_FAVORITES, LOCAL_OTHER]);
        assert_eq!(s[1].title, "Chats");
        assert!(s[1].collapsed);
        assert!(s[1].rooms.is_empty());
        assert_eq!(s[1].total, 1);
    }

    #[test]
    fn changes_only_for_differing_rooms() {
        let rooms = vec![room("a", false, &["3"]), room("b", false, &["4"]), room("c", false, &["3", "4"])];
        let mut ch = tag_changes(&rooms, "3", &["b".into(), "c".into()]);
        ch.sort();
        assert_eq!(ch, [("a".to_string(), vec![]), ("b".to_string(), vec!["4".to_string(), "3".to_string()])]);
    }

    #[test]
    fn tags_parse_and_errors() {
        let t: Vec<ConversationTag> =
            serde_json::from_str(r#"[{"id":"7","name":"Arbeit","sortOrder":1,"collapsed":true,"type":"custom"},{"id":8,"name":"x","type":"other"}]"#)
                .unwrap();
        assert_eq!(t[0].kind, "custom");
        assert!(t[0].collapsed);
        assert_eq!(t[1].id, "8");
        let r: crate::models::Room = serde_json::from_str(r#"{"id":1,"token":"t","type":2,"name":"n","displayName":"N","tagIds":["3",4]}"#).unwrap();
        assert_eq!(r.tag_ids, ["3", "4"]);
        let limit = Error::Http { status: 400, message: r#"{"ocs":{"data":{"error":"limit"}}}"#.into() };
        assert_eq!(category_error_text(CategoryOp::Create, &limit), "Zu viele Kategorien");
        let name = Error::Http { status: 400, message: r#"{"ocs":{"data":{"error":"name"}}}"#.into() };
        assert_eq!(category_error_text(CategoryOp::Rename, &name), "Name ist leer oder schon vergeben");
        assert_eq!(category_error_text(CategoryOp::Delete, &Error::NotFound), "Kategorie nicht gefunden");
    }
}
