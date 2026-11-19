//! Mentions (docs/mentions.md): who can be mentioned in a chat, where a message mentions
//! somebody, and the notation the server reads.
//!
//! The text the user sees and types says `@Lena Brandt`. Which person that is travels
//! beside the text as a [`MentionPick`]; only on the way to the server it becomes
//! `@"lena"` ([`wire_text`]).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::Engine;
use crate::error::Result;
use crate::models::{ChatMessage, RoomType};
use crate::search::fold;

/// How a mention of everybody reads (the server calls it by the conversation's name).
pub const ALL_LABEL: &str = "Alle";
/// The list of a chat is asked for again after this time, in the background.
const REFRESH_AFTER: Duration = Duration::from_secs(120);
/// A longer text after the `@` is no search for a name.
const QUERY_MAX: usize = 40;
/// How far behind the read marker an unseen mention is looked for.
const UNREAD_SCAN: u32 = 2000;

/// A mention the user picked from the suggestions: `@{label}` in the text stands for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionPick {
    /// The display name, without the `@`.
    pub label: String,
    /// What the server reads between the quotes: `lena`, `group/garten`, `all` …
    pub mention_id: String,
}

/// A mention in a message. `label` is how it stands in `MessageView.text` ("@Lena Brandt").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionView {
    pub label: String,
    /// `user`, `group`, `team`, `guest`, `email`, `federated` or `all`.
    pub kind: &'static str,
    /// The user id for `user` (to open the chat with that person), else the mention id.
    pub id: String,
    /// It means the user of this app: their name, or everybody.
    pub is_self: bool,
}

/// One row of the suggestions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionCandidate {
    pub mention_id: String,
    /// The display name, without the `@`.
    pub label: String,
    /// Like [`MentionView::kind`].
    pub kind: &'static str,
    /// For `user`: whose picture the row shows.
    pub user_id: Option<String>,
    /// Second line, empty for people: "Gruppe", "Team", "Gast", "Alle im Chat".
    pub subline: String,
}

/// A message's text for the input field, with its mentions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditDraft {
    pub text: String,
    pub mentions: Vec<MentionPick>,
}

/// One entry of the server's suggestions (`chat/{token}/mentions`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionSuggestion {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub source: String,
    /// Missing on servers before Talk 19.
    #[serde(default)]
    pub mention_id: Option<String>,
}

/// The suggestions of one chat as the server gave them.
pub(crate) struct MentionList {
    at: Instant,
    /// Somebody joined or left since.
    stale: bool,
    items: Arc<Vec<MentionSuggestion>>,
}

/// The kind of a mention parameter (`None`: the parameter is something else).
fn param_kind(param: &Value) -> Option<&'static str> {
    let s = |k: &str| param.get(k).and_then(Value::as_str).unwrap_or_default();
    Some(match s("type") {
        "user" if !s("server").is_empty() => "federated",
        "user" => "user",
        "call" => "all",
        "guest" => "guest",
        "email" => "email",
        "user-group" | "group" => "group",
        "circle" | "team" => "team",
        _ => return None,
    })
}

/// What stands behind the `@` for a parameter of a message; `None` when it is no mention
/// (the acting person of a system message, a file …).
pub(crate) fn param_label(key: &str, param: &Value) -> Option<String> {
    if !key.starts_with("mention-") {
        return None;
    }
    Some(match param_kind(param)? {
        "all" => ALL_LABEL.to_string(),
        _ => param.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

/// What the server reads for a mention parameter. Servers before Talk 19 do not say it.
pub(crate) fn param_mention_id(param: &Value) -> String {
    let s = |k: &str| param.get(k).and_then(Value::as_str).unwrap_or_default();
    if !s("mention-id").is_empty() {
        return s("mention-id").to_string();
    }
    let id = s("id");
    let prefixed = |prefix: &str| match id.starts_with(prefix) {
        true => id.to_string(),
        false => format!("{prefix}{id}"),
    };
    match param_kind(param) {
        Some("all") => "all".into(),
        Some("federated") => format!("federated_user/{id}@{}", s("server")),
        Some("guest") => prefixed("guest/"),
        Some("email") => prefixed("email/"),
        Some("group") => prefixed("group/"),
        Some("team") => prefixed("team/"),
        _ => id.to_string(),
    }
}

/// The kind a mention id stands for.
fn id_kind(mention_id: &str) -> &'static str {
    match mention_id {
        "all" => "all",
        id if id.starts_with("group/") => "group",
        id if id.starts_with("team/") => "team",
        id if id.starts_with("guest/") => "guest",
        id if id.starts_with("email/") => "email",
        id if id.starts_with("federated_user/") => "federated",
        _ => "user",
    }
}

/// The server finds a mention only with these characters between the quotes.
fn wire_id_ok(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || "_-@.' /:".contains(c))
}

/// The text as the server reads it: every `@Label` of a picked mention becomes
/// `@"mention-id"`. Like the server, only after the start, white space or one of `-.'@`;
/// the name must end there (not the start of a longer word). Of two picks with the same
/// name the later one counts. Everything else stays as typed.
pub fn wire_text(text: &str, picks: &[MentionPick]) -> String {
    let mut known: Vec<&MentionPick> = Vec::new();
    for p in picks.iter().rev() {
        if !p.label.is_empty() && wire_id_ok(&p.mention_id) && !known.iter().any(|k| k.label == p.label) {
            known.push(p);
        }
    }
    if known.is_empty() || !text.contains('@') {
        return text.to_string();
    }
    // The longest name first: "Lena Brandt" before "Lena".
    known.sort_by_key(|p| std::cmp::Reverse(p.label.chars().count()));
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    let mut prev: Option<char> = None;
    while let Some(c) = rest.chars().next() {
        if c == '@' && prev.is_none_or(|p| p.is_whitespace() || "-.'@".contains(p)) {
            let after = &rest[1..];
            let hit = known.iter().find(|p| {
                after.starts_with(p.label.as_str())
                    && !after[p.label.len()..].chars().next().is_some_and(char::is_alphanumeric)
            });
            if let Some(p) = hit {
                out.push_str("@\"");
                out.push_str(&p.mention_id);
                out.push('"');
                rest = &after[p.label.len()..];
                prev = Some('"');
                continue;
            }
        }
        out.push(c);
        prev = Some(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// The mentions of a text that is not on the server yet, from what the user picked.
pub(crate) fn pick_views(text: &str, picks: &[MentionPick]) -> Vec<MentionView> {
    let mut out: Vec<MentionView> = Vec::new();
    for p in picks.iter().rev() {
        let label = format!("@{}", p.label);
        if p.label.is_empty() || !text.contains(&label) || out.iter().any(|m| m.label == label) {
            continue;
        }
        out.push(MentionView { label, kind: id_kind(&p.mention_id), id: p.mention_id.clone(), is_self: false });
    }
    out.reverse();
    out
}

impl ChatMessage {
    /// The mention parameters in the order they stand in the text.
    fn mention_params(&self) -> Vec<(&'static str, &Value, String)> {
        let mut found: Vec<(usize, &'static str, &Value, String)> = self
            .message_parameters
            .iter()
            .filter_map(|(key, param)| {
                let label = param_label(key, param)?;
                let at = self.message.find(&format!("{{{key}}}"))?;
                Some((at, param_kind(param)?, param, label))
            })
            .collect();
        found.sort_by_key(|f| f.0);
        found.into_iter().map(|(_, kind, param, label)| (kind, param, label)).collect()
    }

    /// Who this message mentions, in the order of the text, everybody once. `me`: the
    /// user id of this app's user.
    pub fn mentions(&self, me: &str) -> Vec<MentionView> {
        let mut out: Vec<MentionView> = Vec::new();
        for (kind, param, label) in self.mention_params() {
            let id = match kind {
                "user" => param.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                _ => param_mention_id(param),
            };
            let label = format!("@{label}");
            if out.iter().any(|m| m.label == label && m.id == id) {
                continue;
            }
            let is_self = kind == "all" || (kind == "user" && id == me);
            out.push(MentionView { label, kind, id, is_self });
        }
        out
    }

    /// The mentions as picks, for editing the message: with them [`wire_text`] turns the
    /// shown text back into what the server had.
    pub fn mention_picks(&self) -> Vec<MentionPick> {
        let mut out: Vec<MentionPick> = Vec::new();
        for (_, param, label) in self.mention_params() {
            let pick = MentionPick { label, mention_id: param_mention_id(param) };
            if !out.contains(&pick) {
                out.push(pick);
            }
        }
        out
    }
}

/// How well `label` (or the user id) fits what was typed: 0 the name starts with it,
/// 1 a word of the name or the id does, 2 it is somewhere inside.
fn rank(label: &str, id: &str, q: &str) -> Option<u8> {
    if q.is_empty() {
        return Some(0);
    }
    let name = fold(label);
    if name.starts_with(q) {
        return Some(0);
    }
    let word = name.split(|c: char| c.is_whitespace() || "-_.".contains(c)).any(|w| w.starts_with(q));
    if word || fold(id).starts_with(q) {
        return Some(1);
    }
    name.contains(q).then_some(2)
}

/// The rows for what was typed after the `@`, from the server's list of the chat.
fn candidates(items: &[MentionSuggestion], me: &str, query: &str, limit: usize) -> Vec<MentionCandidate> {
    let q = fold(query.trim_start());
    let mut names: HashMap<&str, u32> = HashMap::new();
    for i in items.iter().filter(|i| i.source != "calls") {
        *names.entry(i.label.as_str()).or_default() += 1;
    }
    let mut rows: Vec<(u8, MentionCandidate)> = Vec::new();
    let mut all: Option<MentionCandidate> = None;
    for i in items {
        let mention_id = match (&i.mention_id, i.source.as_str()) {
            (Some(id), _) if !id.is_empty() => id.clone(),
            (_, "federated_users") => format!("federated_user/{}", i.id),
            _ => i.id.clone(),
        };
        if i.source == "calls" {
            if fold(ALL_LABEL).starts_with(&q) {
                all = Some(MentionCandidate {
                    mention_id,
                    label: ALL_LABEL.into(),
                    kind: "all",
                    user_id: None,
                    subline: "Alle im Chat".into(),
                });
            }
            continue;
        }
        if i.label.is_empty() || (i.source == "users" && i.id == me) {
            continue;
        }
        let Some(rank) = rank(&i.label, &i.id, &q) else { continue };
        let (kind, subline) = match i.source.as_str() {
            // Two people with the same name: the id tells them apart.
            "users" if names.get(i.label.as_str()).copied().unwrap_or(0) > 1 => ("user", i.id.clone()),
            "users" => ("user", String::new()),
            "groups" => ("group", "Gruppe".into()),
            "teams" => ("team", "Team".into()),
            "guests" => ("guest", "Gast".into()),
            "emails" => ("email", "Gast".into()),
            "federated_users" => ("federated", i.id.rsplit_once('@').map(|(_, s)| s.to_string()).unwrap_or_default()),
            _ => continue,
        };
        rows.push((
            rank,
            MentionCandidate {
                mention_id,
                label: i.label.clone(),
                kind,
                user_id: (kind == "user").then(|| i.id.clone()),
                subline,
            },
        ));
    }
    // Stable: within a rank the server's order stays (who wrote last comes first).
    rows.sort_by_key(|r| r.0);
    let mut out: Vec<MentionCandidate> = rows.into_iter().map(|r| r.1).collect();
    out.extend(all);
    out.truncate(limit);
    out
}

impl Engine {
    /// Who can be mentioned in a chat, for what was typed after the `@` (empty: everybody).
    /// The list of a chat comes from the server when the chat opens (or with the first
    /// call); after that it is in memory and every call answers at once, while it is
    /// renewed in the background. Empty in a one-to-one chat, without a match, and when
    /// the server cannot be reached.
    pub async fn mention_candidates(&self, token: &str, query: &str, limit: u32) -> Vec<MentionCandidate> {
        let group = self.state_room(token).is_some_and(|r| matches!(r.room_type, RoomType::Group | RoomType::Public));
        if !group || query.chars().count() > QUERY_MAX || query.contains('\n') {
            return Vec::new();
        }
        let known = {
            let lists = self.mention_lists.lock().unwrap();
            lists.get(token).map(|l| (l.items.clone(), l.stale || l.at.elapsed() > REFRESH_AFTER))
        };
        let items = match known {
            Some((items, old)) => {
                if old {
                    self.refresh_mentions(token);
                }
                items
            }
            None => match self.load_mentions(token).await {
                Ok(items) => items,
                Err(e) => {
                    tracing::debug!(error = %e, "no mention suggestions");
                    return Vec::new();
                }
            },
        };
        candidates(&items, &self.user_id(), query, limit as usize)
    }

    async fn load_mentions(&self, token: &str) -> Result<Arc<Vec<MentionSuggestion>>> {
        // One request at a time: quick typing does not ask twice.
        let _one = self.mention_load.lock().await;
        let fresh = |l: &&MentionList| !l.stale && l.at.elapsed() <= REFRESH_AFTER;
        if let Some(l) = self.mention_lists.lock().unwrap().get(token).filter(fresh) {
            return Ok(l.items.clone());
        }
        let items = Arc::new(self.guard(self.client.mention_suggestions(token, "").await)?);
        self.mention_lists
            .lock()
            .unwrap()
            .insert(token.to_string(), MentionList { at: Instant::now(), stale: false, items: items.clone() });
        Ok(items)
    }

    fn refresh_mentions(&self, token: &str) {
        let Some(rt) = self.runtime.get().cloned().or_else(|| tokio::runtime::Handle::try_current().ok()) else { return };
        let weak = self.weak_self();
        let token = token.to_string();
        rt.spawn(async move {
            let Some(engine) = weak.upgrade() else { return };
            let _ = engine.load_mentions(&token).await;
        });
    }

    /// A chat opens: its list is fetched ahead, so the first `@` answers at once.
    pub(crate) fn warm_mentions(&self, token: &str) {
        let group = self.state_room(token).is_some_and(|r| matches!(r.room_type, RoomType::Group | RoomType::Public));
        let fresh = |l: &MentionList| !l.stale && l.at.elapsed() <= REFRESH_AFTER;
        if group && !self.mention_lists.lock().unwrap().get(token).is_some_and(fresh) {
            self.refresh_mentions(token);
        }
    }

    /// Somebody joined or left: the list of this chat is asked for again when it is needed.
    pub(crate) fn mentions_stale(&self, token: &str) {
        if let Some(l) = self.mention_lists.lock().unwrap().get_mut(token) {
            l.stale = true;
        }
    }

    /// The messages behind the read marker that mention the user (their name or
    /// everybody), oldest first, from the cache. The server only says *that* there is
    /// one, and counts groups and teams the user belongs to: when it says so and nothing
    /// direct is found, the first mention of a group or team stands for it.
    pub(crate) fn unread_mentions(&self, token: &str) -> Vec<i64> {
        let Some(room) = self.state_room(token) else { return Vec::new() };
        let flagged = room.unread_mention || room.unread_mention_direct;
        if room.unread_messages == 0 && !flagged {
            return Vec::new();
        }
        let after = room.last_read_message;
        let me = self.user_id();
        let (messages, _) = self.cache.messages_after(token, after, UNREAD_SCAN);
        let mut direct = Vec::new();
        let mut group = None;
        for m in &messages {
            if m.is_deleted() || m.is_system() || self.is_own(&m.actor_type, &m.actor_id) {
                continue;
            }
            let mentions = m.mentions(&me);
            if mentions.iter().any(|x| x.is_self) {
                direct.push(m.id);
            } else if group.is_none() && mentions.iter().any(|x| matches!(x.kind, "group" | "team")) {
                group = Some(m.id);
            }
        }
        if direct.is_empty() && flagged {
            direct.extend(group);
        }
        direct
    }

    /// The one-to-one chat with a person when there is one already (nothing is created;
    /// [`Self::open_direct`] does that).
    pub fn existing_direct(&self, user_id: &str) -> Option<String> {
        self.rooms_snapshot()
            .into_iter()
            .find(|r| r.room_type == RoomType::OneToOne && r.name == user_id)
            .map(|r| r.token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(label: &str, id: &str) -> MentionPick {
        MentionPick { label: label.into(), mention_id: id.into() }
    }

    fn msg(message: &str, params: &str) -> ChatMessage {
        let json = format!(
            r#"{{"id":5,"token":"abc","actorType":"users","actorId":"a","actorDisplayName":"A","timestamp":1,
                "message":"{message}","messageParameters":{params}}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    const PARAMS: &str = r#"{"mention-user1":{"type":"user","id":"lena b","name":"Lena Brandt","mention-id":"lena b"},
        "mention-user2":{"type":"user","id":"mira","name":"Mira Lange","mention-id":"mira"},
        "mention-call1":{"type":"call","id":"abc","name":"Garten","mention-id":"all"},
        "mention-group1":{"type":"user-group","id":"beet","name":"Beetpaten","mention-id":"group/beet"},
        "mention-team1":{"type":"circle","id":"t1","name":"Vorstand","mention-id":"team/t1"},
        "mention-guest1":{"type":"guest","id":"guest/1f","name":"Gast"},
        "mention-federated-user1":{"type":"user","id":"ole","name":"Ole","server":"https://b.example","mention-id":"federated_user/ole@b.example"}}"#;

    #[test]
    fn picked_names_become_the_servers_notation() {
        let picks = [pick("Lena", "lena"), pick("Lena Brandt", "lena b"), pick("Alle", "all")];
        assert_eq!(wire_text("Hallo @Lena Brandt, hallo @Lena.", &picks), r#"Hallo @"lena b", hallo @"lena"."#);
        assert_eq!(wire_text("@Alle\n@Lena", &picks), "@\"all\"\n@\"lena\"");
        // The name goes on: somebody else.
        assert_eq!(wire_text("@Lenas Rad", &picks), "@Lenas Rad");
        // Not picked, in the middle of a word, after a bracket: plain text.
        assert_eq!(wire_text("@Jonas a@Lena (@Lena)", &picks), "@Jonas a@Lena (@Lena)");
        assert_eq!(wire_text("kein Zeichen", &picks), "kein Zeichen");
        // An id the server would not read stays a name instead of showing as `@"…"`.
        assert_eq!(wire_text("@Rüben", &[pick("Rüben", "group/rüben")]), "@Rüben");
        // The same name twice: the later pick counts.
        assert_eq!(wire_text("@Lena", &[pick("Lena", "lena"), pick("Lena", "lena2")]), r#"@"lena2""#);
    }

    #[test]
    fn a_message_tells_who_it_mentions() {
        let m = msg(
            "{mention-user2} und {mention-user1}: {mention-call1} {mention-group1} {mention-team1} {mention-guest1} {mention-federated-user1} {mention-user2}",
            PARAMS,
        );
        assert_eq!(m.rendered_text(), "@Mira Lange und @Lena Brandt: @Alle @Beetpaten @Vorstand @Gast @Ole @Mira Lange");
        let got: Vec<(String, &str, String, bool)> =
            m.mentions("mira").into_iter().map(|x| (x.label, x.kind, x.id, x.is_self)).collect();
        let want = [
            ("@Mira Lange", "user", "mira", true),
            ("@Lena Brandt", "user", "lena b", false),
            ("@Alle", "all", "all", true),
            ("@Beetpaten", "group", "group/beet", false),
            ("@Vorstand", "team", "team/t1", false),
            ("@Gast", "guest", "guest/1f", false),
            ("@Ole", "federated", "federated_user/ole@b.example", false),
        ];
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert_eq!((g.0.as_str(), g.1, g.2.as_str(), g.3), w);
        }
    }

    /// Editing: the shown text with the picks gives back what the server had.
    #[test]
    fn the_shown_text_goes_back_to_the_servers() {
        let m = msg(
            "{mention-user2} und {mention-user1}: {mention-call1} {mention-group1} {mention-team1} {mention-guest1} {mention-federated-user1}",
            PARAMS,
        );
        assert_eq!(wire_text(&m.caption_text(), &m.mention_picks()), m.source_text());
        assert_eq!(
            m.source_text(),
            r#"@"mira" und @"lena b": @"all" @"group/beet" @"team/t1" @"guest/1f" @"federated_user/ole@b.example""#
        );
    }

    /// The acting person of a system message is no mention.
    #[test]
    fn system_messages_mention_nobody() {
        let mut m = msg("{actor} hat {user} hinzugefügt", r#"{"actor":{"type":"user","id":"a","name":"Anna"},"user":{"type":"user","id":"mira","name":"Mira"}}"#);
        m.system_message = "user_added".into();
        assert_eq!(m.rendered_text(), "Anna hat Mira hinzugefügt");
        assert!(m.mentions("mira").is_empty());
    }

    #[test]
    fn a_waiting_message_shows_its_picks() {
        let picks = [pick("Lena", "lena"), pick("Beetpaten", "group/beet"), pick("Weg", "weg")];
        let got = pick_views("@Beetpaten und @Lena", &picks);
        let got: Vec<(&str, &str, &str)> = got.iter().map(|m| (m.label.as_str(), m.kind, m.id.as_str())).collect();
        assert_eq!(got, [("@Lena", "user", "lena"), ("@Beetpaten", "group", "group/beet")]);
    }

    fn item(id: &str, label: &str, source: &str) -> MentionSuggestion {
        let mention_id = match source {
            "federated_users" => None,
            _ => Some(id.to_string()),
        };
        MentionSuggestion { id: id.into(), label: label.into(), source: source.into(), mention_id }
    }

    #[test]
    fn suggestions_are_ranked_and_leave_out_the_user() {
        let items = [
            item("tobias", "Tobias Renz", "users"),
            item("mira", "Mira Lange", "users"),
            item("rene", "René Maurer", "users"),
            item("lena", "Lena Brandt", "users"),
            item("b.renz", "Britta Renz-Ohm", "users"),
            item("group/beet", "Beetpaten", "groups"),
            item("team/t1", "Vorstand", "teams"),
            item("guest/1f", "Renate", "guests"),
            item("ole@b.example", "Ole", "federated_users"),
            item("all", "Garten", "calls"),
        ];
        let labels = |q: &str| -> Vec<String> { candidates(&items, "mira", q, 50).into_iter().map(|c| c.label).collect() };
        // Everybody but the user, in the server's order, everybody last.
        assert_eq!(
            labels(""),
            ["Tobias Renz", "René Maurer", "Lena Brandt", "Britta Renz-Ohm", "Beetpaten", "Vorstand", "Renate", "Ole", "Alle"]
        );
        // The start of the name first, then the start of a word, accents and case aside.
        assert_eq!(labels("REN"), ["René Maurer", "Renate", "Tobias Renz", "Britta Renz-Ohm"]);
        assert_eq!(labels("lena b"), ["Lena Brandt"]);
        assert_eq!(labels("lena x"), Vec::<String>::new());
        assert_eq!(labels("ohm"), ["Britta Renz-Ohm"]);
        assert_eq!(labels("aure"), ["René Maurer"]);
        assert_eq!(labels("b.r"), ["Britta Renz-Ohm"]);
        assert_eq!(labels("al"), ["Alle"]);
        assert_eq!(labels("gar"), Vec::<String>::new());
        assert_eq!(labels("mira"), Vec::<String>::new());

        let all = candidates(&items, "mira", "", 50);
        let by = |label: &str| all.iter().find(|c| c.label == label).unwrap();
        assert_eq!((by("Lena Brandt").kind, by("Lena Brandt").user_id.as_deref(), by("Lena Brandt").subline.as_str()), ("user", Some("lena"), ""));
        assert_eq!((by("Beetpaten").kind, by("Beetpaten").subline.as_str(), by("Beetpaten").mention_id.as_str()), ("group", "Gruppe", "group/beet"));
        assert_eq!((by("Ole").kind, by("Ole").subline.as_str(), by("Ole").mention_id.as_str()), ("federated", "b.example", "federated_user/ole@b.example"));
        assert_eq!((by("Alle").kind, by("Alle").mention_id.as_str(), by("Alle").subline.as_str()), ("all", "all", "Alle im Chat"));
        assert_eq!(candidates(&items, "mira", "", 3).len(), 3);
    }

    #[test]
    fn people_with_the_same_name_show_their_id() {
        let items = [item("lena", "Lena Brandt", "users"), item("lena2", "Lena Brandt", "users")];
        let got: Vec<String> = candidates(&items, "x", "", 50).into_iter().map(|c| c.subline).collect();
        assert_eq!(got, ["lena", "lena2"]);
    }
}
