//! Categories in the chat list: "Favoriten", our own categories, "Chats" (or split into
//! "Gruppen" and "Direktchats"), each one collapsible (Talk calls them conversation tags). The contract for the platforms is
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
/// The section of archived chats in a search; no category, it cannot be collapsed.
const KIND_ARCHIVED: &str = "archived";
/// "Chats" split in two (a device setting): kind and id are the same, collapsed only here.
pub const SECTION_GROUPS: &str = "groups";
pub const SECTION_DIRECT: &str = "direct";

/// How the chats of a section are ordered (Talk's user setting `conversations_sort_order`).
pub const SORT_ACTIVITY: &str = "activity";
pub const SORT_ALPHABETICAL: &str = "alphabetical";
/// The user setting on the server and the capability that reports it.
const SORT_CONFIG_KEY: &str = "conversations_sort_order";

/// What the engine knows about categories.
#[derive(Debug, Default)]
pub(crate) struct Categories {
    /// The server has `conversation-tags`.
    supported: bool,
    /// From the server (cached), any order.
    tags: Vec<ConversationTag>,
    /// Collapsed sections without a server category behind them ([`LOCAL_FAVORITES`],
    /// [`LOCAL_OTHER`], [`SECTION_GROUPS`], [`SECTION_DIRECT`]).
    local_collapsed: HashSet<String>,
    /// Chats by name instead of by last activity.
    alphabetical: bool,
    /// The server keeps the sort order (all devices, Talk-Web); otherwise only here.
    sort_on_server: bool,
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
            alphabetical: cache.meta("sort_order").is_some_and(|v| v == SORT_ALPHABETICAL),
            sort_on_server: cache.meta("sort_on_server").is_some_and(|v| v == "true"),
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
    /// `favorites`, `custom`, `other`, `groups` or `direct`; in a search also `archived`
    /// (archived chats whose name matches; it has no category and is never collapsed).
    pub kind: String,
    /// "Favoriten", the category name or "Chats". Empty when it is the only section:
    /// then show no header (and it is never collapsed).
    pub title: String,
    pub collapsed: bool,
    /// What to show: all chats, or none when collapsed.
    pub rooms: Vec<RoomView>,
    /// All chats in the section (also the hidden ones).
    pub total: u32,
    /// For the head of a collapsed section. Unread messages of all its chats, counted
    /// like the app badge ([`crate::accounts::badge`]): muted chats never, "Nur
    /// Erwähnungen" only with a mention.
    pub unread: u32,
    /// One of its chats has an unread mention (muted chats do not count).
    pub unread_mention: bool,
    /// One of its chats has a running call.
    pub has_call: bool,
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
    /// "Zu Favoriten hinzufügen" / "Aus Favoriten entfernen" (chat menu, docs/settings.md).
    Favorite,
    /// "Als gelesen markieren" (chat menu).
    MarkRead,
    /// "Sortierung" (docs/categories.md).
    Sort,
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
    /// name (then nothing is collapsed). A collapsed section has no rooms; its head shows
    /// what is new in it. `split` (the device setting) puts "Gruppen" and "Direktchats"
    /// where "Chats" is. `_active_token` is no longer used (it once stayed visible).
    pub fn room_sections(&self, query: &str, _active_token: Option<&str>, split: bool) -> Vec<SectionView> {
        let rooms = self.rooms();
        let c = self.categories.lock().unwrap();
        build_sections(rooms, &c.sorted(), &c.local_collapsed, query, split, c.alphabetical)
    }

    /// [`SORT_ACTIVITY`] or [`SORT_ALPHABETICAL`]: how the chats of every section are ordered.
    pub fn sort_order(&self) -> String {
        sort_name(self.categories.lock().unwrap().alphabetical).into()
    }

    /// The sort order from the capabilities (`None`: the server has no such setting, then
    /// ours stays). Sends `RoomsChanged` when the list changes by it.
    pub(crate) fn apply_sort_order(&self, server: Option<&str>) {
        let changed = {
            let mut c = self.categories.lock().unwrap();
            if c.sort_on_server != server.is_some() {
                c.sort_on_server = server.is_some();
                self.cache.set_meta("sort_on_server", c.sort_on_server.to_string());
            }
            match server.map(|v| v == SORT_ALPHABETICAL) {
                Some(alphabetical) if alphabetical != c.alphabetical => {
                    c.alphabetical = alphabetical;
                    self.cache.set_meta("sort_order", sort_name(alphabetical).to_string());
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.emit_rooms();
        }
    }

    /// Chooses the sort order. Saved on the server when it knows the setting (all devices,
    /// Talk-Web), otherwise only here. Shows the change at once and undoes it on failure.
    pub async fn set_sort_order(&self, order: &str) -> Result<()> {
        if order != SORT_ACTIVITY && order != SORT_ALPHABETICAL {
            return Err(Error::Decode(format!("sort order: {order}")));
        }
        let alphabetical = order == SORT_ALPHABETICAL;
        let on_server = {
            let mut c = self.categories.lock().unwrap();
            if c.alphabetical == alphabetical {
                return Ok(());
            }
            c.alphabetical = alphabetical;
            c.sort_on_server
        };
        self.cache.set_meta("sort_order", order.to_string());
        self.emit_rooms();
        if !on_server {
            return Ok(());
        }
        if let Err(e) = self.guard(self.client.set_user_config(SORT_CONFIG_KEY, order).await) {
            self.categories.lock().unwrap().alphabetical = !alphabetical;
            self.cache.set_meta("sort_order", sort_name(!alphabetical).to_string());
            self.emit_rooms();
            return Err(e);
        }
        Ok(())
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

    /// Puts one chat in exactly these own categories (chat menu "Kategorie"). Favorite
    /// state and other chats stay as they are.
    pub async fn set_room_categories(&self, token: &str, ids: &[String]) -> Result<()> {
        if self.state_room(token).is_none() {
            return Err(Error::NotFound);
        }
        let room = self.guard(self.client.set_room_tags(token, ids).await)?;
        self.put_room(room);
        Ok(())
    }

    /// Favorite on or off. Shows at once (`RoomsChanged`); a failure takes it back.
    pub async fn set_favorite(&self, token: &str, on: bool) -> Result<()> {
        if !self.set_local_favorite(token, on)? {
            return Ok(());
        }
        let r = self.guard(self.client.set_favorite(token, on).await);
        if r.is_err() {
            let _ = self.set_local_favorite(token, !on);
        }
        r
    }

    /// Flips the cached favorite flag; `false` when it already was `on`.
    fn set_local_favorite(&self, token: &str, on: bool) -> Result<bool> {
        self.edit_room(token, |r| {
            let changed = r.is_favorite != on;
            r.is_favorite = on;
            changed
        })
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

fn sort_name(alphabetical: bool) -> &'static str {
    if alphabetical {
        SORT_ALPHABETICAL
    } else {
        SORT_ACTIVITY
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

/// One-to-one, former one-to-one, note to self and the changelog: under "Direktchats".
fn is_direct(r: &RoomView) -> bool {
    matches!(r.room_type, 1 | 4 | 5 | 6)
}

/// The sections like Talk-Web: a favorite is under "Favoriten" (and its categories),
/// a chat in no category and no favorite under "Chats", or with `split` under
/// "Gruppen" or "Direktchats". `rooms` come by activity; `alphabetical` orders every
/// section by name instead (case and accents do not count, equal names by activity).
pub(crate) fn build_sections(
    mut rooms: Vec<RoomView>,
    sorted_tags: &[ConversationTag],
    local_collapsed: &HashSet<String>,
    query: &str,
    split: bool,
    alphabetical: bool,
) -> Vec<SectionView> {
    if alphabetical {
        rooms.sort_by_cached_key(|r| (crate::search::fold(&r.name), std::cmp::Reverse(r.last_activity)));
    }
    // Like the message search: case and accents do not count (docs/search.md).
    let q = crate::search::fold(query.trim());
    let searching = !q.is_empty();
    let (archived, rooms): (Vec<RoomView>, Vec<RoomView>) = rooms
        .into_iter()
        .filter(|r| if searching { crate::search::fold(&r.name).contains(&q) } else { !r.is_archived })
        .partition(|r| r.is_archived);

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

    let section = |id: &str, kind: &str, title: String, collapsed: bool, rooms: Vec<RoomView>| SectionView {
        id: id.into(),
        kind: kind.into(),
        title,
        collapsed: collapsed && !searching,
        total: rooms.len() as u32,
        unread: crate::accounts::badge(&rooms).messages,
        unread_mention: rooms.iter().any(|r| r.unread_messages > 0 && r.unread_mention && r.notification_level != 3),
        has_call: rooms.iter().any(|r| r.has_call),
        rooms,
    };
    let mut sections: Vec<SectionView> = Vec::new();
    for t in tags {
        let members: Vec<RoomView> = rooms
            .iter()
            .filter(|r| match t.kind.as_str() {
                KIND_FAVORITES => r.is_favorite,
                KIND_OTHER => !r.is_favorite && !has_category(r, &custom),
                _ => r.tag_ids.contains(&t.id),
            })
            .cloned()
            .collect();
        if split && t.kind == KIND_OTHER {
            let (direct, groups): (Vec<RoomView>, Vec<RoomView>) = members.into_iter().partition(is_direct);
            for (id, title, part) in [(SECTION_GROUPS, "Gruppen", groups), (SECTION_DIRECT, "Direktchats", direct)] {
                if !part.is_empty() {
                    sections.push(section(id, id, title.into(), local_collapsed.contains(id), part));
                }
            }
        } else if !members.is_empty() {
            sections.push(section(&t.id, &t.kind, title(t), t.collapsed, members));
        }
    }

    if let [only] = sections.as_mut_slice() {
        only.title.clear();
        only.collapsed = false;
    }
    // Archived chats are only in the list while their name is searched for.
    if !archived.is_empty() {
        sections.push(SectionView {
            id: KIND_ARCHIVED.into(),
            kind: KIND_ARCHIVED.into(),
            title: "Archiviert".into(),
            collapsed: false,
            total: archived.len() as u32,
            unread: 0,
            unread_mention: false,
            has_call: false,
            rooms: archived,
        });
    }
    // Collapsed means empty: what is new stands in the head (unread, mention, call).
    for s in &mut sections {
        if s.collapsed {
            s.rooms.clear();
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

/// German message for a failed category or chat-menu action.
pub fn category_error_text(op: CategoryOp, e: &Error) -> String {
    use CategoryOp::*;
    let text = match (op, e) {
        (_, Error::Network(_) | Error::RetryLater { .. }) => "Keine Verbindung",
        (Create, Error::Http { status: 400, message }) if message.contains("limit") => "Zu viele Kategorien",
        (Create | Rename, Error::Http { status: 400, .. }) => "Name ist leer oder schon vergeben",
        (Favorite | MarkRead, Error::NotFound) => "Chat nicht gefunden",
        (_, Error::NotFound) => "Kategorie nicht gefunden",
        (Collapse, _) => "Konnte nicht gespeichert werden",
        (Sort, _) => "Sortierung konnte nicht gespeichert werden",
        (Create, _) => "Kategorie konnte nicht erstellt werden",
        (Rename | Assign | Favorite | MarkRead, _) => "Speichern fehlgeschlagen",
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
            can_chat: true,
            can_delete_others: false,
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
        let s = build_sections(rooms, &server_tags(), &HashSet::new(), "", false, false);
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
        let s = build_sections(vec![room("a", false, &[])], &server_tags(), &HashSet::new(), "", false, false);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "");
        let mut tags = server_tags();
        tags[3].collapsed = true;
        let s = build_sections(vec![room("a", false, &[])], &tags, &HashSet::new(), "", false, false);
        assert!(!s[0].collapsed);
        assert_eq!(ids(&s[0]), ["a"]);
    }

    #[test]
    fn collapsed_is_empty_and_the_head_counts() {
        let mut tags = server_tags();
        tags[1].collapsed = true;
        let mut unread = room("u", false, &["3"]);
        unread.unread_messages = 2;
        let mut mention = room("m", false, &["3"]);
        (mention.unread_messages, mention.unread_mention, mention.notification_level) = (3, true, 2);
        // "Nur Erwähnungen" without a mention and "Aus" do not count, like the app badge.
        let mut quiet = room("q", false, &["3"]);
        (quiet.unread_messages, quiet.notification_level) = (5, 2);
        let mut muted = room("off", false, &["3"]);
        (muted.unread_messages, muted.unread_mention, muted.notification_level) = (7, true, 3);
        let mut call = room("k", false, &["3"]);
        call.has_call = true;
        let rooms = vec![room("x", false, &["3"]), unread, mention, quiet, muted.clone(), call, room("d", false, &[])];
        let s = build_sections(rooms.clone(), &tags, &HashSet::new(), "", false, false);
        assert!(s[0].collapsed);
        assert_eq!(s[0].total, 6);
        assert!(s[0].rooms.is_empty());
        assert_eq!((s[0].unread, s[0].unread_mention, s[0].has_call), (5, true, true));
        assert_eq!((s[1].unread, s[1].unread_mention, s[1].has_call), (0, false, false));
        // A muted chat alone shows nothing in the head.
        let s = build_sections(vec![muted, room("d", false, &[])], &tags, &HashSet::new(), "", false, false);
        assert_eq!((s[0].unread, s[0].unread_mention), (0, false));
        // Searching opens everything.
        let s = build_sections(rooms, &tags, &HashSet::new(), "x", false, false);
        assert!(!s[0].collapsed);
        assert_eq!(ids(&s[0]), ["x"]);
    }

    #[test]
    fn local_sections_without_server() {
        let collapsed: HashSet<String> = [LOCAL_OTHER.to_string()].into();
        let mut archived = room("z", false, &[]);
        archived.is_archived = true;
        let rooms = vec![room("a", true, &[]), room("b", false, &["3"]), archived];
        // A search finds the archived chat too, under its own heading, and ignores case.
        let found = build_sections(rooms.clone(), &[], &collapsed, "Z", false, false);
        assert_eq!(found.iter().map(|s| (s.kind.as_str(), s.title.as_str())).collect::<Vec<_>>(), [("archived", "Archiviert")]);
        assert_eq!(found[0].rooms[0].token, "z");
        let s = build_sections(rooms, &[], &collapsed, "", false, false);
        assert_eq!(s.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), [LOCAL_FAVORITES, LOCAL_OTHER]);
        assert_eq!(s[1].title, "Chats");
        assert!(s[1].collapsed);
        assert!(s[1].rooms.is_empty());
        assert_eq!(s[1].total, 1);
    }

    fn typed(token: &str, room_type: u8, fav: bool) -> RoomView {
        RoomView { room_type, ..room(token, fav, &[]) }
    }

    #[test]
    fn split_into_groups_and_direct() {
        let mut in_cat = typed("k", 1, false);
        in_cat.tag_ids = vec!["3".into()];
        let rooms = vec![
            typed("g", 2, false),
            typed("p", 3, false),
            typed("d", 1, false),
            typed("former", 5, false),
            typed("note", 6, false),
            typed("log", 4, false),
            typed("fav", 1, true), // only under "Favoriten"
            in_cat,                // only under "Arbeit"
        ];
        let s = build_sections(rooms.clone(), &server_tags(), &HashSet::new(), "", true, false);
        let heads: Vec<(&str, &str, &str)> = s.iter().map(|s| (s.id.as_str(), s.kind.as_str(), s.title.as_str())).collect();
        assert_eq!(
            heads,
            [("1", "favorites", "Favoriten"), ("3", "custom", "Arbeit"), ("groups", "groups", "Gruppen"), ("direct", "direct", "Direktchats")]
        );
        assert_eq!(ids(&s[0]), ["fav"]);
        assert_eq!(ids(&s[1]), ["k"]);
        assert_eq!(ids(&s[2]), ["g", "p"]);
        assert_eq!(ids(&s[3]), ["d", "former", "note", "log"]);
        // The same without the setting: one "Chats" as before.
        let off = build_sections(rooms, &server_tags(), &HashSet::new(), "", false, false);
        assert_eq!(off.last().map(|s| s.title.as_str()), Some("Chats"));
        assert_eq!(off.last().unwrap().total, 6);
    }

    #[test]
    fn split_collapses_locally_and_keeps_rules() {
        let collapsed: HashSet<String> = [SECTION_GROUPS.to_string()].into();
        let mut unread = typed("u", 2, false);
        unread.unread_messages = 1;
        let rooms = vec![typed("g", 2, false), unread, typed("act", 2, false), typed("d", 1, false)];
        // Also without server categories (local "fav"/"other").
        let s = build_sections(rooms.clone(), &[], &collapsed, "", true, false);
        assert_eq!(s.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), [SECTION_GROUPS, SECTION_DIRECT]);
        assert!(s[0].collapsed);
        assert_eq!(s[0].total, 3);
        assert!(s[0].rooms.is_empty());
        assert_eq!(s[0].unread, 1);
        assert!(!s[1].collapsed);
        // The server's collapsed "Chats" does not close the split sections.
        let mut tags = server_tags();
        tags[3].collapsed = true;
        let s = build_sections(rooms.clone(), &tags, &HashSet::new(), "", true, false);
        assert!(s.iter().all(|s| !s.collapsed));
        // Searching opens everything.
        let s = build_sections(rooms.clone(), &[], &collapsed, "g", true, false);
        assert_eq!(s.len(), 1);
        assert!(!s[0].collapsed);
        // Only one kind left: no heading, never collapsed.
        let s = build_sections(vec![typed("g", 2, false)], &[], &collapsed, "", true, false);
        assert_eq!((s.len(), s[0].title.as_str(), s[0].collapsed), (1, "", false));
    }

    #[test]
    fn alphabetical_in_every_section() {
        let named = |token: &str, name: &str, activity: i64, fav: bool, tags: &[&str]| RoomView {
            name: name.into(),
            last_activity: activity,
            ..room(token, fav, tags)
        };
        // As the engine hands them over: favorites first, then by activity.
        let rooms = vec![
            named("z", "Zeltlager", 9, true, &[]),
            named("a", "anna", 1, true, &[]),
            named("o", "Övelgönne", 8, false, &["3"]),
            named("u", "Ulm", 7, false, &["3"]),
            named("e2", "Élan", 6, false, &[]),
            named("b", "Berta", 5, false, &[]),
            named("e1", "elan", 2, false, &[]),
        ];
        let by_activity = build_sections(rooms.clone(), &server_tags(), &HashSet::new(), "", false, false);
        assert_eq!(ids(&by_activity[0]), ["z", "a"]);
        assert_eq!(ids(&by_activity[2]), ["e2", "b", "e1"]);
        let s = build_sections(rooms.clone(), &server_tags(), &HashSet::new(), "", false, true);
        assert_eq!(s.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Favoriten", "Arbeit", "Chats"]);
        // Case and accents do not count; equal names by activity.
        assert_eq!(ids(&s[0]), ["a", "z"]);
        assert_eq!(ids(&s[1]), ["o", "u"]);
        assert_eq!(ids(&s[2]), ["b", "e2", "e1"]);
        // Also in a search.
        let s = build_sections(rooms, &server_tags(), &HashSet::new(), "l", false, true);
        assert_eq!(s.iter().flat_map(ids).collect::<Vec<_>>(), ["z", "o", "u", "e2", "e1"]);
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
