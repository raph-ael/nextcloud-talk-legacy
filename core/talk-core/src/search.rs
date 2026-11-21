//! Full-text search over the messages of all conversations (docs/search.md).
//!
//! The index is its own SQLite file next to the cache. The cache keeps the newest part of
//! each chat for showing it; the index keeps the words of the whole history, and only
//! those: text, sender and time. It answers while the user types, without the server.
//!
//! - `docs`: one row per message that says something.
//! - `fts`: trigram index over the folded text (lowercase, no accents), so a part of a
//!   word finds the word.
//! - `cover`: the id ranges of a chat that are indexed without a gap. What is missing
//!   follows from it, and a worker fetches it in the background ([`Engine::search_step`]).
//! - `older`: where the server ended the last page below a range. The server pages by
//!   time, and that is not always the order of the ids (imported history).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use unicode_normalization::char::{decompose_canonical, is_combining_mark};

use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::models::ChatMessage;

/// Any change of the tables: the index starts empty and is fetched again.
const SCHEMA: i32 = 1;
/// Messages indexed per conversation. Beyond that the server's search has to do.
pub(crate) const MAX_PER_ROOM: i64 = 20_000;
/// Messages per request of the background worker (the server's limit).
const SYNC_PAGE: u32 = 200;
/// Break between two requests of the worker.
const SYNC_PAUSE: Duration = Duration::from_millis(150);
/// The worker looks for new work this often when nothing wakes it.
const SYNC_IDLE: Duration = Duration::from_secs(60);
/// The worker starts this long after the engine: the first screen comes first.
const SYNC_DELAY: Duration = Duration::from_secs(3);
const MAX_WORDS: usize = 8;
/// A word shorter than this cannot use the trigram index.
const INDEXED_CHARS: usize = 3;
/// Trigrams of a search handed to the index; more do not narrow it down further.
const MAX_TRIGRAMS: usize = 24;
/// Characters of a snippet before its first hit, and its length. A hit this near the
/// beginning is in sight anyway (two lines of a list): then nothing is cut off.
const SNIPPET_NEAR: usize = 40;
const SNIPPET_LEAD: usize = 24;
const SNIPPET_CHARS: usize = 160;
/// Hits asked from the server per request (its limit) and requests per search.
const REMOTE_PAGE: u32 = 25;
const REMOTE_PAGES: u32 = 3;

/// A piece of a snippet; `hit` marks a search word.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnippetPart {
    pub text: String,
    pub hit: bool,
}

/// A message that matches the search.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub token: String,
    pub message_id: i64,
    pub timestamp: i64,
    pub room_name: String,
    /// The sender, as in `MessageView` (for the picture).
    pub actor_type: String,
    pub actor_id: String,
    pub actor_name: String,
    pub is_own: bool,
    /// Show the sender before the snippet: own messages ("Du") and messages in groups.
    pub show_actor: bool,
    /// One line around the first hit, cut with "…" where it leaves text out.
    pub snippet: Vec<SnippetPart>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    /// Newest first.
    pub hits: Vec<SearchHit>,
    /// In one chat: all matches, also beyond `hits` ("3 von 17"). In all chats: as many
    /// as `hits`.
    pub total: u32,
    /// There are more matches than `hits`: ask again with a higher limit.
    pub more: bool,
    /// Everything that can match is on the device: no need to ask the server.
    pub complete: bool,
    /// Conversations whose history is loaded, and all of them. While they differ, the
    /// history is still being fetched.
    pub chats_done: u32,
    pub chats_total: u32,
}

/// How far the index has come.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatus {
    pub chats_done: u32,
    pub chats_total: u32,
    /// Conversations with more messages than the index keeps.
    pub chats_capped: u32,
    pub messages: u32,
}

// --- folding and queries -----------------------------------------------------------------

/// What the search compares: lowercase and without accents ("Hüpfburg" → "hupfburg").
pub(crate) fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        fold_char(c, &mut out);
    }
    out
}

fn fold_char(c: char, out: &mut String) {
    if c.is_ascii() {
        out.push(c.to_ascii_lowercase());
        return;
    }
    for lower in c.to_lowercase() {
        decompose_canonical(lower, |d| {
            if !is_combining_mark(d) {
                out.push(d);
            }
        });
    }
}

/// The words of a search text, folded. Every one of them has to occur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Query {
    words: Vec<String>,
}

impl Query {
    /// `None`: nothing to search for yet (less than two characters).
    pub fn parse(text: &str) -> Option<Self> {
        let mut words: Vec<String> = text.split_whitespace().map(fold).filter(|w| !w.is_empty()).collect();
        // The longest first: it narrows the most. A word inside another one adds nothing.
        words.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        let mut kept: Vec<String> = Vec::new();
        for w in words {
            if kept.len() < MAX_WORDS && !kept.iter().any(|k| k.contains(&w)) {
                kept.push(w);
            }
        }
        let chars: usize = kept.iter().map(|w| w.chars().count()).sum();
        (chars >= 2).then_some(Self { words: kept })
    }

    fn matches(&self, folded: &str) -> bool {
        self.words.iter().all(|w| folded.contains(w.as_str()))
    }

    /// The FTS5 expression that narrows the search down: the trigrams of the longer
    /// words, all of which a match contains (somewhere: the index keeps no positions, so
    /// the words themselves are checked on what it lets through). `None`: only short words.
    fn fts(&self) -> Option<String> {
        let mut tokens: Vec<String> = Vec::new();
        for w in &self.words {
            let chars: Vec<char> = w.chars().collect();
            for tri in chars.windows(INDEXED_CHARS) {
                let token = format!("\"{}\"", tri.iter().collect::<String>().replace('"', "\"\""));
                if tokens.len() < MAX_TRIGRAMS && !tokens.contains(&token) {
                    tokens.push(token);
                }
            }
        }
        (!tokens.is_empty()).then(|| tokens.join(" AND "))
    }

    /// One line of `text` around the first hit, with the hits marked.
    pub fn snippet(&self, text: &str) -> Vec<SnippetPart> {
        let chars: Vec<char> = text.chars().map(|c| if c.is_whitespace() || c.is_control() { ' ' } else { c }).collect();
        // Folded text, and for each of its bytes the character it came from.
        let mut folded = String::with_capacity(text.len());
        let mut origin: Vec<usize> = Vec::with_capacity(text.len());
        for (i, c) in chars.iter().enumerate() {
            let before = folded.len();
            fold_char(*c, &mut folded);
            origin.extend(std::iter::repeat_n(i, folded.len() - before));
        }
        let mut marked = vec![false; chars.len()];
        for w in &self.words {
            let mut from = 0;
            while let Some(at) = folded[from..].find(w.as_str()) {
                let (start, end) = (from + at, from + at + w.len());
                marked[origin[start]..=origin[end - 1]].fill(true);
                from = end;
            }
        }
        let first = marked.iter().position(|m| *m).unwrap_or(0);
        let mut start = if first <= SNIPPET_NEAR { 0 } else { first - SNIPPET_LEAD };
        if start > 0 {
            // Not in the middle of a word, if a space is near.
            if let Some(space) = chars[start..first].iter().position(|c| *c == ' ') {
                start += space + 1;
            }
        }
        let end = (start + SNIPPET_CHARS).min(chars.len());
        let mut parts: Vec<SnippetPart> = Vec::new();
        let mut push = |text: &str, hit: bool| match parts.last_mut() {
            Some(last) if last.hit == hit => last.text.push_str(text),
            _ => parts.push(SnippetPart { text: text.to_string(), hit }),
        };
        if start > 0 {
            push("…", false);
        }
        let mut buf = [0u8; 4];
        for (c, hit) in chars[start..end].iter().zip(&marked[start..end]) {
            push(c.encode_utf8(&mut buf), *hit);
        }
        if end < chars.len() {
            push("…", false);
        }
        parts
    }

    /// The word the server is asked for (it knows one sequence of characters only), as
    /// the user wrote it.
    fn remote_term(text: &str) -> Option<String> {
        text.split_whitespace().max_by_key(|w| w.chars().count()).map(str::to_string)
    }
}

/// What the search looks at in a message: its text or caption, and the name of what it
/// shares (file, contact; a place and a poll are their name already). Empty: not searchable.
pub(crate) fn search_text(m: &ChatMessage) -> String {
    if m.is_system() || m.is_deleted() {
        return String::new();
    }
    let mut text = m.caption_text().trim().to_string();
    if let Some(file) = m.attachment().filter(|f| !f.voice_message) {
        let name = if file.contact_name.is_empty() { file.name.trim() } else { file.contact_name.trim() };
        if !name.is_empty() && !text.contains(name) {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(name);
        }
    }
    text
}

// --- the index ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Doc {
    pub id: i64,
    pub ts: i64,
    pub actor_type: String,
    pub actor_id: String,
    pub actor: String,
    pub text: String,
    pub expires: i64,
}

/// What a search found.
#[derive(Debug, Default)]
pub(crate) struct Matches {
    /// Newest first.
    pub found: Vec<Found>,
    /// In one chat: all matches. Everywhere: as many as `found`.
    pub total: usize,
    /// There are more matches than `found`.
    pub more: bool,
}

/// A row that matched.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub token: String,
    pub id: i64,
    pub ts: i64,
    pub actor_type: String,
    pub actor_id: String,
    pub actor: String,
    pub text: String,
}

enum Op {
    /// Messages to index, ids that say nothing (any more), and the range they fill.
    Put {
        token: String,
        docs: Vec<Doc>,
        gone: Vec<i64>,
        cover: Option<(i64, i64)>,
    },
    Capped(String),
    /// Below the range that starts at `below`, the next page starts after message `id`.
    Older {
        token: String,
        below: i64,
        id: i64,
    },
    Forget(String),
    /// Forget every conversation that is not in the list.
    Keep(Vec<String>),
    Purge(i64),
    Clear,
    Flush(tokio::sync::oneshot::Sender<()>),
}

struct Inner {
    read: Mutex<Connection>,
    tx: Mutex<mpsc::Sender<Op>>,
}

/// Without a data directory every call is a no-op (nothing is found).
pub(crate) struct Index {
    inner: Option<Inner>,
}

impl Index {
    pub fn open(dir: Option<&Path>) -> Self {
        let inner = dir.and_then(|dir| match Inner::open(dir) {
            Ok(i) => Some(i),
            Err(e) => {
                tracing::warn!("search index unavailable: {e}");
                None
            }
        });
        Self { inner }
    }

    pub fn enabled(&self) -> bool {
        self.inner.is_some()
    }

    fn send(&self, op: Op) {
        if let Some(i) = &self.inner {
            let _ = i.tx.lock().unwrap().send(op);
        }
    }

    /// Waits until all writes sent so far are committed.
    pub async fn flush(&self) {
        if self.inner.is_none() {
            return;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.send(Op::Flush(tx));
        let _ = rx.await;
    }

    fn read<T: Default>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> T {
        let Some(i) = &self.inner else { return T::default() };
        let conn = i.read.lock().unwrap();
        f(&conn).unwrap_or_else(|e| {
            tracing::warn!("search index read failed: {e}");
            T::default()
        })
    }

    /// Indexes `messages` and the new versions of earlier ones (`updates`). `cover`: the
    /// ids from..=to of this chat are all among them (or say nothing).
    pub fn put(&self, token: &str, messages: &[ChatMessage], updates: &[ChatMessage], cover: Option<(i64, i64)>) {
        if self.inner.is_none() || (messages.is_empty() && updates.is_empty() && cover.is_none()) {
            return;
        }
        let (mut docs, mut gone) = (Vec::new(), Vec::new());
        for m in messages.iter().chain(updates) {
            let text = search_text(m);
            if m.id <= 0 {
                continue;
            }
            if text.is_empty() {
                gone.push(m.id);
                continue;
            }
            docs.push(Doc {
                id: m.id,
                ts: m.timestamp,
                actor_type: m.actor_type.clone(),
                actor_id: m.actor_id.clone(),
                actor: m.actor_display_name.clone(),
                text,
                expires: m.expiration_timestamp,
            });
        }
        self.send(Op::Put { token: token.to_string(), docs, gone, cover: cover.filter(|(lo, hi)| lo <= hi) });
    }

    pub fn set_capped(&self, token: &str) {
        self.send(Op::Capped(token.to_string()));
    }

    /// The server ended the page below the range starting at `below` with message `id`.
    pub fn set_older(&self, token: &str, below: i64, id: i64) {
        self.send(Op::Older { token: token.to_string(), below, id });
    }

    /// Everything a step of the worker goes by, after all writes: two steps in a row that
    /// start from the same answer got nowhere.
    pub(crate) async fn progress(&self, token: &str, look: bool) -> (bool, Vec<(i64, i64)>, Option<i64>, i64) {
        self.flush().await;
        let ranges = self.cover(token);
        let older = ranges.first().and_then(|(lo, _)| self.older(token, *lo));
        (look, ranges, older, self.count(token))
    }

    /// The message the next older page starts after, if the last page was fetched for the
    /// range that starts at `below`. A range that grew since by other means (it met a
    /// lower one) starts over at its lowest id.
    pub fn older(&self, token: &str, below: i64) -> Option<i64> {
        self.read(|c| {
            c.prepare_cached("SELECT id FROM older WHERE token = ?1 AND below = ?2")?
                .query_row(params![token, below], |r| r.get(0))
                .optional()
        })
    }

    /// The conversation is gone or its history was cleared.
    pub fn forget(&self, token: &str) {
        self.send(Op::Forget(token.to_string()));
    }

    pub fn keep(&self, tokens: Vec<String>) {
        self.send(Op::Keep(tokens));
    }

    /// Drops messages that have expired.
    pub fn purge(&self, now: i64) {
        self.send(Op::Purge(now));
    }

    pub fn clear(&self) {
        self.send(Op::Clear);
    }

    /// The indexed ranges of a chat, the newest first.
    pub fn cover(&self, token: &str) -> Vec<(i64, i64)> {
        self.read(|c| {
            let mut st = c.prepare_cached("SELECT lo, hi FROM cover WHERE token = ?1 ORDER BY hi DESC")?;
            let rows = st.query_map([token], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    /// The ranges of all chats.
    fn covers(&self) -> HashMap<String, Vec<(i64, i64)>> {
        self.read(|c| {
            let mut st = c.prepare_cached("SELECT token, lo, hi FROM cover ORDER BY hi DESC")?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?;
            let mut out: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
            for row in rows {
                let (token, range) = row?;
                out.entry(token).or_default().push(range);
            }
            Ok(out)
        })
    }

    fn capped(&self) -> HashSet<String> {
        self.read(|c| {
            let mut st = c.prepare_cached("SELECT token FROM capped")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect()
        })
    }

    pub fn count(&self, token: &str) -> i64 {
        self.read(|c| c.query_row("SELECT count(*) FROM docs WHERE token = ?1", [token], |r| r.get(0)))
    }

    fn count_all(&self) -> i64 {
        self.read(|c| c.query_row("SELECT count(*) FROM docs", [], |r| r.get(0)))
    }

    pub fn has(&self, token: &str, id: i64) -> bool {
        self.read(|c| {
            c.query_row("SELECT 1 FROM docs WHERE token = ?1 AND id = ?2", params![token, id], |_| Ok(true))
                .optional()
                .map(|found| found.unwrap_or(false))
        })
    }

    /// The newest `limit` matches.
    ///
    /// In one chat every message of it is looked at (a chat is small, and the count of all
    /// matches is wanted there). Everywhere the trigram index narrows down, in the order
    /// of time, and the search stops when it has enough: a word that is in every other
    /// message answers as fast as one that is nowhere.
    pub fn search(&self, q: &Query, token: Option<&str>, limit: usize, now: i64) -> Matches {
        use rusqlite::types::Value;
        const COLUMNS: &str = "d.token, d.id, d.ts, d.actor_type, d.actor_id, d.actor, d.text";
        fn found(r: &rusqlite::Row) -> rusqlite::Result<Found> {
            Ok(Found {
                token: r.get(0)?,
                id: r.get(1)?,
                ts: r.get(2)?,
                actor_type: r.get(3)?,
                actor_id: r.get(4)?,
                actor: r.get(5)?,
                text: r.get(6)?,
            })
        }
        let mut params: Vec<Value> = vec![Value::Integer(now)];
        let mut conditions = String::from("(d.expires = 0 OR d.expires > ?1)");
        for w in &q.words {
            params.push(Value::Text(w.clone()));
            conditions.push_str(&format!(" AND instr(d.fold, ?{}) > 0", params.len()));
        }
        self.read(|c| match token {
            Some(token) => {
                params.push(Value::Text(token.to_string()));
                let sql = format!("SELECT {COLUMNS} FROM docs d WHERE d.token = ?{} AND {conditions} ORDER BY d.id DESC", params.len());
                let mut st = c.prepare_cached(&sql)?;
                let mut rows = st.query(rusqlite::params_from_iter(params))?;
                let mut out = Matches::default();
                while let Some(row) = rows.next()? {
                    out.total += 1;
                    if out.found.len() < limit {
                        out.found.push(found(row)?);
                    }
                }
                out.more = out.total > out.found.len();
                Ok(out)
            }
            None => {
                let from = match q.fts() {
                    Some(expr) => {
                        params.push(Value::Text(expr));
                        // The index first, in its own order (newest first), then the row.
                        format!("FROM fts CROSS JOIN docs d ON d.rowid = fts.rowid WHERE fts MATCH ?{} AND {conditions} ORDER BY fts.rowid DESC", params.len())
                    }
                    None => format!("FROM docs d WHERE {conditions} ORDER BY d.rowid DESC"),
                };
                params.push(Value::Integer(limit as i64 + 1));
                let mut st = c.prepare_cached(&format!("SELECT {COLUMNS} {from} LIMIT ?{}", params.len()))?;
                let rows = st.query_map(rusqlite::params_from_iter(params), found)?;
                let mut hits = rows.collect::<rusqlite::Result<Vec<_>>>()?;
                let more = hits.len() > limit;
                hits.truncate(limit);
                Ok(Matches { total: hits.len(), found: hits, more })
            }
        })
    }
}

impl Inner {
    fn open(dir: &Path) -> rusqlite::Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let file = dir.join("search.sqlite");
        let write = match open_db(&file) {
            Ok(c) => c,
            Err(e) => {
                // Unreadable: it is only an index, start over.
                tracing::warn!("search index unreadable ({e}), starting empty");
                for suffix in ["", "-wal", "-shm"] {
                    let _ = std::fs::remove_file(dir.join(format!("search.sqlite{suffix}")));
                }
                open_db(&file)?
            }
        };
        let read = Connection::open(&file)?;
        read.busy_timeout(Duration::from_secs(5))?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("talk-search".into())
            .spawn(move || writer(write, rx))
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Ok(Self { read: Mutex::new(read), tx: Mutex::new(tx) })
    }
}

fn open_db(file: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(file)?;
    c.busy_timeout(Duration::from_secs(5))?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    c.pragma_update(None, "synchronous", "NORMAL")?;
    let version: i32 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version != SCHEMA {
        c.execute_batch(
            "DROP TABLE IF EXISTS docs;
             DROP TABLE IF EXISTS fts;
             DROP TABLE IF EXISTS cover;
             DROP TABLE IF EXISTS capped;
             DROP TABLE IF EXISTS older;
             CREATE TABLE docs (
                 rowid INTEGER PRIMARY KEY, token TEXT NOT NULL, id INTEGER NOT NULL, ts INTEGER NOT NULL,
                 actor_type TEXT NOT NULL, actor_id TEXT NOT NULL, actor TEXT NOT NULL,
                 text TEXT NOT NULL, fold TEXT NOT NULL, expires INTEGER NOT NULL,
                 UNIQUE (token, id)
             );
             CREATE INDEX docs_expires ON docs (expires) WHERE expires > 0;
             CREATE VIRTUAL TABLE fts USING fts5(fold, content='', contentless_delete=1, detail=none, tokenize='trigram');
             CREATE TABLE cover (
                 token TEXT NOT NULL, lo INTEGER NOT NULL, hi INTEGER NOT NULL,
                 PRIMARY KEY (token, lo)
             ) WITHOUT ROWID;
             CREATE TABLE capped (token TEXT PRIMARY KEY) WITHOUT ROWID;",
        )?;
        c.pragma_update(None, "user_version", SCHEMA)?;
    }
    // Added later, and nothing else depends on it: an index made before keeps its content.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS older (
             token TEXT PRIMARY KEY, below INTEGER NOT NULL, id INTEGER NOT NULL
         ) WITHOUT ROWID;",
    )?;
    // Fails loudly on a corrupt file instead of on the first real query.
    c.query_row("SELECT count(*) FROM cover", [], |_| Ok(()))?;
    Ok(c)
}

/// The row of a message: rows are in the order of time, so the newest matches are the
/// first ones the index hands out and a search can stop early. The low bits keep the
/// messages of one second apart (ids of one server never collide; with a federated
/// conversation they could, then the next free row is taken).
fn time_rowid(ts: i64, id: i64) -> i64 {
    (ts.clamp(0, (1 << 40) - 1) << 22) | (id & 0x3F_FFFF)
}

fn writer(mut conn: Connection, rx: mpsc::Receiver<Op>) {
    while let Ok(op) = rx.recv() {
        if let Op::Flush(done) = op {
            let _ = done.send(());
            continue;
        }
        if let Err(e) = apply(&mut conn, op) {
            tracing::warn!("search index write failed: {e}");
        }
    }
}

fn remove(tx: &rusqlite::Transaction, rowid: i64) -> rusqlite::Result<()> {
    tx.prepare_cached("DELETE FROM fts WHERE rowid = ?1")?.execute([rowid])?;
    tx.prepare_cached("DELETE FROM docs WHERE rowid = ?1")?.execute([rowid])?;
    Ok(())
}

fn remove_where(tx: &rusqlite::Transaction, condition: &str, param: &dyn rusqlite::ToSql) -> rusqlite::Result<()> {
    let rowids: Vec<i64> = {
        let mut st = tx.prepare(&format!("SELECT rowid FROM docs WHERE {condition}"))?;
        st.query_map([param], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    for rowid in rowids {
        remove(tx, rowid)?;
    }
    Ok(())
}

fn forget(tx: &rusqlite::Transaction, token: &str) -> rusqlite::Result<()> {
    remove_where(tx, "token = ?1", &token)?;
    tx.execute("DELETE FROM cover WHERE token = ?1", [token])?;
    tx.execute("DELETE FROM capped WHERE token = ?1", [token])?;
    tx.execute("DELETE FROM older WHERE token = ?1", [token])?;
    Ok(())
}

/// Adds a range and joins it with the ones it touches.
fn cover_add(tx: &rusqlite::Transaction, token: &str, mut lo: i64, mut hi: i64) -> rusqlite::Result<()> {
    let touching: Vec<(i64, i64)> = {
        let mut st = tx.prepare_cached("SELECT lo, hi FROM cover WHERE token = ?1 AND lo <= ?3 + 1 AND hi >= ?2 - 1")?;
        st.query_map(params![token, lo, hi], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    for (l, h) in touching {
        tx.execute("DELETE FROM cover WHERE token = ?1 AND lo = ?2", params![token, l])?;
        lo = lo.min(l);
        hi = hi.max(h);
    }
    tx.execute("INSERT INTO cover (token, lo, hi) VALUES (?1, ?2, ?3)", params![token, lo, hi])?;
    Ok(())
}

fn apply(conn: &mut Connection, op: Op) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    match op {
        Op::Flush(_) => {}
        Op::Put { token, docs, gone, cover } => {
            for d in &docs {
                let old: Option<(i64, String, i64, String, i64)> = tx
                    .prepare_cached("SELECT rowid, text, ts, actor, expires FROM docs WHERE token = ?1 AND id = ?2")?
                    .query_row(params![token, d.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
                    .optional()?;
                match old {
                    // The usual case for the last message of the chat list: nothing to write.
                    Some((_, text, ts, actor, expires)) if (&text, ts, &actor, expires) == (&d.text, d.ts, &d.actor, d.expires) => {}
                    Some((rowid, text, ..)) => {
                        let folded = fold(&d.text);
                        tx.prepare_cached("UPDATE docs SET ts = ?2, actor = ?3, text = ?4, fold = ?5, expires = ?6 WHERE rowid = ?1")?
                            .execute(params![rowid, d.ts, d.actor, d.text, folded, d.expires])?;
                        if text != d.text {
                            tx.prepare_cached("DELETE FROM fts WHERE rowid = ?1")?.execute([rowid])?;
                            tx.prepare_cached("INSERT INTO fts (rowid, fold) VALUES (?1, ?2)")?.execute(params![rowid, folded])?;
                        }
                    }
                    None => {
                        let folded = fold(&d.text);
                        let mut insert = tx.prepare_cached(
                            "INSERT OR IGNORE INTO docs (rowid, token, id, ts, actor_type, actor_id, actor, text, fold, expires)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        )?;
                        // The next free row from its place in time on.
                        let mut rowid = time_rowid(d.ts, d.id);
                        while insert.execute(params![rowid, token, d.id, d.ts, d.actor_type, d.actor_id, d.actor, d.text, folded, d.expires])? == 0 {
                            rowid += 1;
                        }
                        tx.prepare_cached("INSERT INTO fts (rowid, fold) VALUES (?1, ?2)")?.execute(params![rowid, folded])?;
                    }
                }
            }
            for id in gone {
                let rowid: Option<i64> = tx
                    .prepare_cached("SELECT rowid FROM docs WHERE token = ?1 AND id = ?2")?
                    .query_row(params![token, id], |r| r.get(0))
                    .optional()?;
                if let Some(rowid) = rowid {
                    remove(&tx, rowid)?;
                }
            }
            if let Some((lo, hi)) = cover {
                cover_add(&tx, &token, lo, hi)?;
            }
        }
        Op::Capped(token) => {
            tx.execute("INSERT OR IGNORE INTO capped (token) VALUES (?1)", [token])?;
        }
        Op::Older { token, below, id } => {
            tx.prepare_cached("INSERT OR REPLACE INTO older (token, below, id) VALUES (?1, ?2, ?3)")?
                .execute(params![token, below, id])?;
        }
        Op::Forget(token) => forget(&tx, &token)?,
        Op::Keep(tokens) => {
            let keep: HashSet<&str> = tokens.iter().map(String::as_str).collect();
            let known: Vec<String> = {
                let mut st = tx.prepare("SELECT DISTINCT token FROM docs UNION SELECT token FROM cover")?;
                st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
            };
            for token in known.iter().filter(|t| !keep.contains(t.as_str())) {
                forget(&tx, token)?;
            }
        }
        Op::Purge(now) => remove_where(&tx, "expires > 0 AND expires <= ?1", &now)?,
        Op::Clear => {
            tx.execute_batch("DELETE FROM docs; DELETE FROM fts; DELETE FROM cover; DELETE FROM capped; DELETE FROM older;")?;
        }
    }
    tx.commit()
}

// --- the engine's side -------------------------------------------------------------------

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The history of a chat is on the device: one range from its beginning. Its newest
/// messages may be a few seconds behind (they come with the next look at the chat list).
fn loaded(ranges: Option<&Vec<(i64, i64)>>) -> bool {
    matches!(ranges.map(Vec::as_slice), Some([(0, _)]))
}

/// What a step of the worker found to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// A request was made; there may be more to fetch.
    Fetched,
    /// Nothing is missing (or the chat is as full as the index gets).
    Done,
}

impl Engine {
    /// Hands what the engine learns about a chat on to the index. A cleared history
    /// starts the chat over.
    pub(crate) fn index_messages(&self, token: &str, messages: &[ChatMessage], updates: &[ChatMessage], cover: Option<(i64, i64)>) {
        if !self.search.enabled() {
            return;
        }
        if messages.iter().any(|m| m.system_message == "history_cleared") {
            self.search.forget(token);
            self.search_wake.notify_one();
            return;
        }
        self.search.put(token, messages, updates, cover);
    }

    /// The conversation list changed: its last messages are searchable at once, and
    /// conversations that are gone leave the index.
    pub(crate) fn index_rooms(&self, rooms: &[crate::models::Room], full: bool) {
        if !self.search.enabled() {
            return;
        }
        for r in rooms {
            if let Some(m) = &r.last_message {
                self.search.put(&r.token, std::slice::from_ref(m), &[], None);
            }
        }
        if full {
            self.search.keep(rooms.iter().map(|r| r.token.clone()).collect());
        }
        self.search_wake.notify_one();
    }

    /// Searches the messages on the device, in one chat (`token`) or in all of them. Does
    /// not wait for anything: call it with every change of the search text.
    pub fn search_messages(&self, query: &str, token: Option<&str>, limit: u32) -> SearchResults {
        let status = self.search_scope(token);
        let mut out = SearchResults {
            complete: status.chats_done == status.chats_total && status.chats_capped == 0,
            chats_done: status.chats_done,
            chats_total: status.chats_total,
            ..Default::default()
        };
        let Some(q) = Query::parse(query) else { return out };
        let matches = self.search.search(&q, token, limit as usize, now());
        let rooms = self.search_rooms();
        out.total = matches.total as u32;
        out.more = matches.more;
        out.hits = matches.found.into_iter().filter_map(|f| self.search_hit(&q, &rooms, f)).collect();
        out
    }

    /// Like [`Self::search_messages`], with what only the server knows added: while the
    /// history is still being fetched, and for chats too long for the index. Ask it when
    /// `complete` is false and the user stopped typing.
    pub async fn search_messages_everywhere(&self, query: &str, token: Option<&str>, limit: u32) -> Result<SearchResults> {
        let mut out = self.search_messages(query, token, limit);
        let (Some(q), Some(term)) = (Query::parse(query), Query::remote_term(query)) else { return Ok(out) };
        if out.complete || !self.search.enabled() {
            return Ok(out);
        }
        let rooms = self.search_rooms();
        let mut known: HashSet<(String, i64)> = out.hits.iter().map(|h| (h.token.clone(), h.message_id)).collect();
        let mut extra = Vec::new();
        let mut cursor = 0;
        for _ in 0..REMOTE_PAGES {
            let page = self.guard(self.client.search_messages(&term, token, cursor, REMOTE_PAGE).await)?;
            let n = page.len() as u32;
            for e in page {
                // What the index holds, it knows better: the server also matches in data
                // that is not text.
                if known.contains(&(e.token.clone(), e.message_id)) || self.search.has(&e.token, e.message_id) {
                    continue;
                }
                if !q.matches(&fold(&e.text)) {
                    continue;
                }
                let Some((room_name, is_group)) = rooms.get(&e.token) else { continue };
                let is_own = self.is_own(&e.actor_type, &e.actor_id);
                known.insert((e.token.clone(), e.message_id));
                extra.push(SearchHit {
                    snippet: q.snippet(&e.text),
                    actor_name: remote_actor(&e.title, room_name, &e.actor_id),
                    actor_type: e.actor_type,
                    actor_id: e.actor_id,
                    room_name: room_name.clone(),
                    show_actor: is_own || *is_group,
                    is_own,
                    timestamp: e.timestamp,
                    message_id: e.message_id,
                    token: e.token,
                });
            }
            cursor += REMOTE_PAGE;
            if n < REMOTE_PAGE {
                break;
            }
        }
        out.total += extra.len() as u32;
        out.hits.extend(extra);
        out.hits.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(b.message_id.cmp(&a.message_id)));
        if out.hits.len() > limit as usize {
            out.hits.truncate(limit as usize);
            out.more = true;
        }
        if token.is_none() {
            out.total = out.hits.len() as u32;
        }
        Ok(out)
    }

    /// How far the history of all conversations is loaded.
    pub fn search_status(&self) -> SearchStatus {
        let mut status = self.search_scope(None);
        status.messages = self.search.count_all() as u32;
        status
    }

    fn search_scope(&self, token: Option<&str>) -> SearchStatus {
        if !self.search.enabled() {
            return SearchStatus::default();
        }
        let tokens: Vec<String> = match token {
            Some(t) => vec![t.to_string()],
            None => self.search_rooms().into_keys().collect(),
        };
        let (covers, capped) = (self.search.covers(), self.search.capped());
        let mut status = SearchStatus { chats_total: tokens.len() as u32, ..Default::default() };
        for t in &tokens {
            if capped.contains(t) {
                status.chats_capped += 1;
                status.chats_done += 1;
            } else if loaded(covers.get(t)) {
                status.chats_done += 1;
            }
        }
        status
    }

    /// Name and "is a group" of every conversation.
    fn search_rooms(&self) -> HashMap<String, (String, bool)> {
        self.rooms_snapshot().into_iter().map(|r| (r.token.clone(), (r.display_name.clone(), crate::notices::is_group(&r)))).collect()
    }

    fn search_hit(&self, q: &Query, rooms: &HashMap<String, (String, bool)>, f: Found) -> Option<SearchHit> {
        let (room_name, is_group) = rooms.get(&f.token)?;
        let is_own = self.is_own(&f.actor_type, &f.actor_id);
        Some(SearchHit {
            snippet: q.snippet(&f.text),
            room_name: room_name.clone(),
            actor_type: f.actor_type,
            actor_id: f.actor_id,
            actor_name: f.actor,
            show_actor: is_own || *is_group,
            is_own,
            timestamp: f.ts,
            message_id: f.id,
            token: f.token,
        })
    }

    /// Starts fetching the history of all conversations for the search, in the background.
    pub(crate) fn spawn_search_sync(self: &Arc<Self>) {
        if !self.search.enabled() {
            return;
        }
        let weak = Arc::downgrade(self);
        let wake = self.search_wake.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(SYNC_DELAY).await;
            // Conversations the server refused: not again in this run.
            let mut skip: HashSet<String> = HashSet::new();
            // Conversations asked once for what changed while the app was not running.
            let mut looked: HashSet<String> = HashSet::new();
            loop {
                let Some(engine) = weak.upgrade() else { return };
                engine.search.purge(now());
                let mut rooms = engine.rooms_snapshot();
                rooms.sort_by_key(|r| std::cmp::Reverse(r.last_activity));
                drop(engine);
                'rooms: for room in &rooms {
                    if skip.contains(&room.token) {
                        continue;
                    }
                    // What the index knew of the chat before the last request.
                    let mut before = None;
                    loop {
                        let Some(engine) = weak.upgrade() else { return };
                        // A call needs the line and the processor.
                        if engine.calls.busy_anywhere() {
                            drop(engine);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                        let look = looked.insert(room.token.clone());
                        // A request that left the index as it was would be made again and
                        // again, every 150 ms: whatever the reason, this chat waits for
                        // the next run of the app.
                        let known = engine.search.progress(&room.token, look).await;
                        if before.as_ref() == Some(&known) {
                            tracing::warn!(token = %room.token, "search: the last request changed nothing, giving up on this chat");
                            skip.insert(room.token.clone());
                            break;
                        }
                        before = Some(known);
                        match engine.search_step(&room.token, look).await {
                            Ok(Step::Done) => break,
                            Ok(Step::Fetched) => {}
                            Err(Error::Unauthorized) => return,
                            // Offline: the next round tries again.
                            Err(Error::Network(_) | Error::RetryLater { .. }) => {
                                looked.remove(&room.token);
                                break 'rooms;
                            }
                            Err(e) => {
                                tracing::debug!(token = %room.token, "search: history not readable: {e}");
                                skip.insert(room.token.clone());
                                break;
                            }
                        }
                        drop(engine);
                        tokio::time::sleep(SYNC_PAUSE).await;
                    }
                }
                tokio::select! {
                    _ = wake.notified() => {}
                    _ = tokio::time::sleep(SYNC_IDLE) => {}
                }
            }
        });
        if let Some(old) = self.search_task.lock().unwrap().replace(task) {
            old.abort();
        }
    }

    /// One request towards a complete index of this chat: the newest page of a chat that
    /// is new to the index, then what came since, then the gaps below, down to the
    /// beginning.
    ///
    /// `look`: ask for what came since even if the chat list shows nothing new. An edit or
    /// a deletion is a hidden message, the list does not announce it. Once per run of the
    /// app; after that such a change arrives with the next message of the chat.
    pub(crate) async fn search_step(&self, token: &str, look: bool) -> Result<Step> {
        self.search.flush().await;
        let target = self.state_room(token).and_then(|r| r.last_message.map(|m| m.id)).unwrap_or(0);
        let ranges = self.search.cover(token);
        let Some(&(lo, hi)) = ranges.first() else {
            let batch = self.guard(self.client.history(token, None, SYNC_PAGE).await)?;
            let ids = batch.messages.iter().map(|m| m.id);
            match (ids.clone().min(), ids.max()) {
                (Some(oldest), Some(newest)) => {
                    let oldest = batch.last_given.map_or(oldest, |given| given.min(oldest));
                    self.index_batch(token, &batch.messages, Some((oldest, newest.max(target))));
                    if let Some(end) = page_end(&batch) {
                        self.search.set_older(token, oldest, end);
                    }
                }
                // An empty chat.
                _ => self.search.put(token, &[], &[], Some((0, target))),
            }
            return Ok(Step::Fetched);
        };
        if hi < target || look {
            let batch = self.guard(self.client.newer_messages(token, hi, SYNC_PAGE as usize - 1).await)?;
            let newest = batch.messages.iter().map(|m| m.id).max().into_iter().chain(batch.last_given).max();
            match newest {
                Some(newest) => self.index_batch(token, &batch.messages, Some((hi + 1, newest))),
                // The list names a message the chat does not hand out: nothing to wait for.
                None => self.search.put(token, &[], &[], Some((hi + 1, target))),
            }
            return Ok(Step::Fetched);
        }
        if lo == 0 {
            return Ok(Step::Done);
        }
        if self.search.capped().contains(token) {
            return Ok(Step::Done);
        }
        if self.search.count(token) >= MAX_PER_ROOM {
            self.search.set_capped(token);
            return Ok(Step::Done);
        }
        // The server hands out the page before a message in time. Mostly that is the order
        // of the ids too. In a chat with imported history it is not: an older message can
        // have a higher id. So the next page starts where the server ended the last one,
        // not at the lowest id (which asked for the same page for ever).
        let from = self.search.older(token, lo).unwrap_or(lo);
        let batch = self.guard(self.client.history(token, Some(from), SYNC_PAGE).await)?;
        let Some(end) = page_end(&batch).filter(|end| *end != from) else {
            // Nothing older: this is the beginning.
            self.index_batch(token, &batch.messages, Some((0, lo - 1)));
            return Ok(Step::Fetched);
        };
        let ids = batch.messages.iter().map(|m| m.id);
        let lowest = ids.clone().min().map_or(end, |id| id.min(end));
        let highest = ids.max().map_or(end, |id| id.max(end));
        if highest < lo {
            // In the order of the ids as well: the range grows downwards.
            self.index_batch(token, &batch.messages, Some((lowest, lo - 1)));
            self.search.set_older(token, lowest, end);
        } else {
            // Ids from above the range: the page says nothing about what lies below it.
            // The range stays, the walk goes on in the server's order to the beginning.
            self.index_batch(token, &batch.messages, None);
            self.search.set_older(token, lo, end);
        }
        Ok(Step::Fetched)
    }

    /// A page of history with what it says about earlier messages (edits, deletions).
    fn index_batch(&self, token: &str, messages: &[ChatMessage], cover: Option<(i64, i64)>) {
        let updates = crate::engine::parent_updates(messages);
        self.index_messages(token, messages, &updates, cover);
    }
}

/// The message a page of history ends with on the server: the oldest one in time. The
/// server names it (it may be one the user does not get to see).
fn page_end(batch: &crate::client::ChatBatch) -> Option<i64> {
    batch.last_given.or_else(|| batch.messages.iter().min_by_key(|m| (m.timestamp, m.id)).map(|m| m.id))
}

/// The sender of a hit from the server. It only gives a title, "{user}" in a one-to-one
/// chat, else "{user} in {conversation}" in the language of the account.
fn remote_actor(title: &str, room_name: &str, actor_id: &str) -> String {
    let title = title.trim();
    let name = match title.rfind(room_name).filter(|at| *at > 0 && !room_name.is_empty()) {
        // Without the conversation and the word that joins the two.
        Some(at) => title[..at].trim_end().rsplit_once(char::is_whitespace).map_or("", |(name, _)| name).trim(),
        None => title,
    };
    if name.is_empty() { actor_id.to_string() } else { name.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: i64, text: &str) -> ChatMessage {
        serde_json::from_value(serde_json::json!({
            "id": id, "token": "t", "actorType": "users", "actorId": "jonas", "actorDisplayName": "Jonas",
            "timestamp": 1000 + id, "message": text, "messageType": "comment"
        }))
        .unwrap()
    }

    fn dir_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("talk-search-test-{name}-{}", std::process::id()))
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = dir_path(name);
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn ids(index: &Index, query: &str, token: Option<&str>) -> Vec<i64> {
        let q = Query::parse(query).expect("a query");
        index.search(&q, token, 50, 0).found.into_iter().map(|f| f.id).collect()
    }

    fn text(parts: &[SnippetPart]) -> String {
        parts.iter().map(|p| if p.hit { format!("[{}]", p.text) } else { p.text.clone() }).collect()
    }

    /// A chat as a server hands it out: pages of three, the newest first, in the order
    /// of time. `ids` are the messages from the oldest to the newest.
    fn chat_server(ids: &'static [i64]) -> crate::testing::Server {
        use crate::testing::{Reply, param};
        crate::testing::serve(move |path| {
            if !path.contains("/chat/t?") || param(path, "lookIntoFuture").as_deref() != Some("0") {
                return Reply::status(304);
            }
            let known = param(path, "lastKnownMessageId").and_then(|id| id.parse::<i64>().ok());
            let end = match known {
                Some(id) => ids.iter().position(|i| *i == id).unwrap_or(0),
                None => ids.len(),
            };
            let page: Vec<_> = (end.saturating_sub(3)..end).rev().collect();
            let Some(last) = page.last() else { return Reply::status(304) };
            let messages: Vec<_> = page
                .iter()
                .map(|at| {
                    serde_json::json!({
                        "id": ids[*at], "token": "t", "actorType": "users", "actorId": "jonas", "actorDisplayName": "Jonas",
                        "timestamp": 1000 + *at as i64, "message": format!("Nachricht {}", ids[*at]), "messageType": "comment"
                    })
                })
                .collect();
            Reply::ocs(messages.into()).header("X-Chat-Last-Given", ids[*last])
        })
    }

    /// Runs the worker's steps on chat "t" until nothing is missing.
    async fn fill(engine: &Engine) -> usize {
        for steps in 0..50 {
            if engine.search_step("t", false).await.unwrap() == Step::Done {
                return steps;
            }
        }
        panic!("the worker asks for ever");
    }

    #[tokio::test]
    async fn history_in_the_order_of_the_ids_is_fetched_page_by_page() {
        let server = chat_server(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let engine = crate::testing::engine(&server, Some(crate::testing::data_dir("search-plain")));
        // Three pages and the request that finds nothing older.
        assert_eq!(fill(&engine).await, 4);
        assert_eq!((engine.search.count("t"), engine.search.cover("t")), (8, vec![(0, 8)]));
        let asked: Vec<_> = server.seen("/chat/t").iter().map(|p| crate::testing::param(p, "lastKnownMessageId")).collect();
        assert_eq!(asked, [None, Some("6".into()), Some("3".into()), Some("1".into())]);
    }

    #[tokio::test]
    async fn imported_history_reaches_the_beginning() {
        // The oldest messages were imported later: they have higher ids than the ones
        // that follow them in time. The page before message 10 holds ids above it only.
        let server = chat_server(&[20, 21, 22, 23, 24, 25, 10, 11, 12, 13, 30, 31, 32, 33]);
        let engine = crate::testing::engine(&server, Some(crate::testing::data_dir("search-imported")));
        assert_eq!(fill(&engine).await, 6);
        assert_eq!((engine.search.count("t"), engine.search.cover("t")), (14, vec![(0, 33)]));
        // Every page once, each from where the server ended the one before.
        let asked: Vec<_> = server.seen("/chat/t").iter().map(|p| crate::testing::param(p, "lastKnownMessageId")).collect();
        let from = |id: i64| Some(id.to_string());
        assert_eq!(asked, [None, from(31), from(12), from(25), from(22), from(20)]);
    }

    #[tokio::test]
    async fn the_walk_through_imported_history_goes_on_after_a_restart() {
        let server = chat_server(&[20, 21, 22, 23, 24, 25, 10, 11, 12, 13, 30, 31, 32, 33]);
        let dir = crate::testing::data_dir("search-restart");
        {
            let engine = crate::testing::engine(&server, Some(dir.clone()));
            for _ in 0..4 {
                assert_eq!(engine.search_step("t", false).await.unwrap(), Step::Fetched);
            }
            engine.search.flush().await;
        }
        let engine = crate::testing::engine(&server, Some(dir));
        assert_eq!(fill(&engine).await, 2);
        assert_eq!((engine.search.count("t"), engine.search.cover("t")), (14, vec![(0, 33)]));
        assert_eq!(server.seen("/chat/t").len(), 6);
    }

    #[test]
    fn folding_ignores_case_and_accents() {
        assert_eq!(fold("Hüpfburg-Verleih ÉCOLE Ångström"), "hupfburg-verleih ecole angstrom");
        assert_eq!(fold("Straße İstanbul"), "straße istanbul");
        assert!(Query::parse("a").is_none());
        assert!(Query::parse("  ").is_none());
        assert_eq!(Query::parse("Ab").unwrap().words, ["ab"]);
        // The longest first; a word inside another one is dropped.
        assert_eq!(Query::parse("zu Hüpfburg burg").unwrap().words, ["hupfburg", "zu"]);
    }

    #[test]
    fn a_snippet_marks_the_words() {
        let q = Query::parse("hupf morgen").unwrap();
        assert_eq!(text(&q.snippet("Die Hüpfburg kommt\nmorgen um zehn")), "Die [Hüpf]burg kommt [morgen] um zehn");
        let long = format!("{} die Hüpfburg {}", "Anfang ".repeat(20), "Ende ".repeat(60));
        let parts = q.snippet(&long);
        let line = text(&parts);
        assert!(line.starts_with("…Anfang "), "{line}");
        assert!(line.contains("die [Hüpf]burg") && line.ends_with('…'), "{line}");
        assert!(parts.iter().map(|p| p.text.chars().count()).sum::<usize>() <= SNIPPET_CHARS + 2);
        // No hit in the text (a hit from the server in other data): the beginning.
        assert_eq!(text(&q.snippet("Nichts davon")), "Nichts davon");
    }

    #[tokio::test]
    async fn finds_parts_of_words_in_any_order() {
        let index = Index::open(Some(&dir("find")));
        index.put(
            "t",
            &[
                msg(1, "Die Hüpfburg kommt morgen"),
                msg(2, "Morgen bringt Karla die HÜPFBURG"),
                msg(3, "Wer holt den Kuchen?"),
                msg(4, "Zu spät"),
            ],
            &[],
            Some((1, 4)),
        );
        index.put("u", &[msg(5, "Hupfburg ohne Punkte")], &[], None);
        index.flush().await;
        assert_eq!(ids(&index, "burg", None), [5, 2, 1]);
        assert_eq!(ids(&index, "hüpf", Some("t")), [2, 1]);
        assert_eq!(ids(&index, "morgen karla", None), [2]);
        assert_eq!(ids(&index, "karla MORGEN", None), [2]);
        // Two characters, alone and next to a longer word.
        assert_eq!(ids(&index, "zu", None), [4]);
        assert_eq!(ids(&index, "ho ku", None), [3]);
        assert_eq!(ids(&index, "die ko", None), [1]);
        assert_eq!(ids(&index, "gibtsnicht", None), Vec::<i64>::new());
        // Quotes are characters like any other.
        assert_eq!(ids(&index, "\"x\"", None).len(), 0);
        // A chat counts beyond the limit; everywhere only says that there is more.
        let q = Query::parse("burg").unwrap();
        let m = index.search(&q, Some("t"), 1, 0);
        assert_eq!((m.found.len(), m.total, m.more), (1, 2, true));
        let m = index.search(&q, None, 2, 0);
        assert_eq!((m.found.len(), m.total, m.more), (2, 2, true));
        assert!(!index.search(&q, None, 3, 0).more);
        // The letters of a word somewhere in the message are not the word.
        assert_eq!(ids(&index, "burgmor", None), Vec::<i64>::new());
        assert_eq!(index.cover("t"), [(1, 4)]);
        assert!(index.has("t", 3) && !index.has("t", 9));
    }

    #[tokio::test]
    async fn edits_deletions_and_expiry_reach_the_index() {
        let index = Index::open(Some(&dir("change")));
        let mut expiring = msg(3, "Nur kurz hier");
        expiring.expiration_timestamp = 500;
        index.put("t", &[msg(1, "Treffen am Dienstag"), msg(2, "Bitte löschen"), expiring], &[], None);
        index.flush().await;
        assert_eq!(ids(&index, "dienstag", None), [1]);

        let mut deleted = msg(2, "Message deleted by author");
        deleted.message_type = "comment_deleted".into();
        index.put("t", &[], &[msg(1, "Treffen am Mittwoch"), deleted], None);
        index.flush().await;
        assert_eq!(ids(&index, "dienstag", None), Vec::<i64>::new());
        assert_eq!(ids(&index, "mittwoch", None), [1]);
        assert_eq!(ids(&index, "löschen", None), Vec::<i64>::new());

        // Expired messages are not found, and the purge removes them.
        let q = Query::parse("kurz").unwrap();
        assert_eq!(index.search(&q, None, 10, 400).total, 1);
        assert_eq!(index.search(&q, None, 10, 600).total, 0);
        index.purge(600);
        index.flush().await;
        assert_eq!(index.count("t"), 1);

        // A system message says nothing.
        let mut system = msg(7, "{actor} hat die Unterhaltung erstellt");
        system.system_message = "conversation_created".into();
        index.put("t", &[system], &[], None);
        index.flush().await;
        assert_eq!(index.count("t"), 1);
    }

    #[tokio::test]
    async fn ranges_join_and_conversations_are_forgotten() {
        let index = Index::open(Some(&dir("cover")));
        index.put("t", &[msg(50, "oben")], &[], Some((40, 50)));
        index.put("t", &[msg(10, "unten")], &[], Some((0, 12)));
        index.put("u", &[msg(60, "anderswo")], &[], Some((60, 60)));
        index.flush().await;
        assert_eq!(index.cover("t"), [(40, 50), (0, 12)]);
        assert!(!loaded(index.covers().get("t")));
        // Touching ranges join, also across the hole.
        index.put("t", &[], &[], Some((51, 70)));
        index.put("t", &[], &[], Some((13, 39)));
        index.flush().await;
        assert_eq!(index.cover("t"), [(0, 70)]);
        assert!(loaded(index.covers().get("t")));

        index.keep(vec!["t".into()]);
        index.flush().await;
        assert!(index.cover("u").is_empty());
        assert_eq!(ids(&index, "anderswo", None), Vec::<i64>::new());
        index.forget("t");
        index.flush().await;
        assert_eq!((index.count("t"), index.cover("t").len()), (0, 0));
        assert_eq!(ids(&index, "oben", None), Vec::<i64>::new());
    }

    #[tokio::test]
    async fn the_index_survives_a_restart() {
        let d = dir("restart");
        {
            let index = Index::open(Some(&d));
            index.put("t", &[msg(1, "Bleibt da")], &[], Some((0, 1)));
            index.flush().await;
        }
        let index = Index::open(Some(&d));
        assert_eq!(ids(&index, "bleibt", None), [1]);
        assert_eq!(index.cover("t"), [(0, 1)]);
        // Without a directory nothing is kept and nothing is found.
        let none = Index::open(None);
        none.put("t", &[msg(1, "Bleibt da")], &[], None);
        none.flush().await;
        assert!(!none.enabled() && ids(&none, "bleibt", None).is_empty());
    }

    /// How long a search takes with a large history. Not part of the normal run:
    /// `cargo test --release -p talk-core search::tests::speed -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn speed() {
        const WORDS: [&str; 24] = [
            "Hofladen", "Lieferung", "morgen", "Kühlwagen", "Gemüse", "Abrechnung", "bitte", "der", "die", "und", "Bestellung",
            "Anhänger", "Markt", "Äpfel", "Kisten", "heute", "nicht", "wir", "kommt", "später", "Schlüssel", "Büro", "danke", "noch",
        ];
        let index = Index::open(Some(&dir("speed")));
        let mut seed = 7u64;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        let started = std::time::Instant::now();
        for chat in 0..50 {
            let batch: Vec<ChatMessage> = (0..1000)
                .map(|i| {
                    let text: Vec<&str> = (0..40).map(|_| WORDS[next() % WORDS.len()]).collect();
                    msg(chat * 1000 + i + 1, &text.join(" "))
                })
                .collect();
            index.put(&format!("chat{chat}"), &batch, &[], Some((0, (chat + 1) * 1000)));
        }
        index.put("chat7", &[msg(900_001, "Die Quittenmarmelade steht im Regal")], &[], None);
        index.flush().await;
        println!("50.000 Nachrichten indiziert in {:?}", started.elapsed());
        for query in ["quittenmarmelade", "quitt", "schlüssel büro", "der", "de", "kühlwagen morgen anhänger", "gibtesnicht", "zz"] {
            let q = Query::parse(query).unwrap();
            let mut best = Duration::MAX;
            let mut found = 0;
            for _ in 0..5 {
                let t = std::time::Instant::now();
                found = index.search(&q, None, 50, 0).total;
                best = best.min(t.elapsed());
            }
            let t = std::time::Instant::now();
            let in_chat = index.search(&q, Some("chat7"), 500, 0).total;
            println!("{query:28} {found:6} Treffer in {best:?}; im Chat {in_chat:4} in {:?}", t.elapsed());
        }
        let file = std::fs::metadata(dir_path("speed").join("search.sqlite")).map_or(0, |m| m.len());
        println!("Datei: {} MB", file / 1_048_576);
    }

    #[test]
    fn files_and_contacts_are_found_by_name() {
        let file = |name: &str, mime: &str, caption: &str, extra: serde_json::Value| -> ChatMessage {
            let mut param = serde_json::json!({ "type": "file", "id": "7", "name": name, "mimetype": mime });
            param.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            let mut m = msg(1, caption);
            m.message_parameters.insert("file".into(), param);
            m
        };
        assert_eq!(search_text(&file("Bericht.pdf", "application/pdf", "{file}", serde_json::json!({}))), "Bericht.pdf");
        assert_eq!(
            search_text(&file("Bericht.pdf", "application/pdf", "Zahlen fürs Fest", serde_json::json!({}))),
            "Zahlen fürs Fest\nBericht.pdf"
        );
        assert_eq!(
            search_text(&file("x.vcf", "text/vcard", "{file}", serde_json::json!({ "contact-name": "Karla Brandt" }))),
            "Karla Brandt"
        );
        let mut voice = file("talk-recording.mp3", "audio/mpeg", "{file}", serde_json::json!({}));
        voice.message_type = "voice-message".into();
        assert_eq!(search_text(&voice), "");
    }

    #[test]
    fn the_sender_of_a_server_hit_comes_from_its_title() {
        assert_eq!(remote_actor("Jonas Weber in Sommerfest", "Sommerfest", "jonas"), "Jonas Weber");
        assert_eq!(remote_actor("Jonas Weber dans Sommerfest", "Sommerfest", "jonas"), "Jonas Weber");
        assert_eq!(remote_actor("Jonas Weber", "Jonas Weber", "jonas"), "Jonas Weber");
        assert_eq!(remote_actor("", "Sommerfest", "jonas"), "jonas");
    }
}
