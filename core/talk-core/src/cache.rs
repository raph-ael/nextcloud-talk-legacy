//! Local cache of conversations and messages (SQLite) plus avatars and previews (files),
//! so the UI starts from disk and works offline.
//!
//! Server objects are stored as their JSON so the schema stays stable while the wire
//! models grow. Per conversation the cache holds one gap-free range of messages: from the
//! newest synced message back to `oldest_id` (hidden system messages included). Messages
//! outside that range are never stored, which keeps "everything between A and B is known"
//! true and lets the engine fetch only the delta when a chat opens.
//!
//! Reads are synchronous on a dedicated connection (fast first frame). Writes go to a
//! writer thread in order, one transaction per batch; [`Cache::flush`] waits for them.
//! It's a cache: on a schema change or a broken file it starts empty.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::models::{ChatMessage, Room};

/// Bump on any schema or stored-JSON change: old caches are dropped.
const SCHEMA: i32 = 1;
/// Messages kept per conversation (the newest ones).
pub(crate) const MAX_PER_ROOM: i64 = 2000;
/// Size limit of the avatar/preview file cache.
const MEDIA_LIMIT: u64 = 100 * 1024 * 1024;

/// The cached message range of one conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChatRange {
    pub oldest_id: i64,
    pub newest_id: i64,
    /// Older messages exist on the server below `oldest_id`.
    pub has_more: bool,
    pub last_common_read: Option<i64>,
}

enum Op {
    Meta(&'static str, String),
    /// All rooms (`full`: drop rooms not in the list, with their messages) or changed ones.
    Rooms { rooms: Vec<Room>, full: bool },
    /// Newest page of a chat, replacing whatever was cached.
    ReplaceChat {
        token: String,
        messages: Vec<ChatMessage>,
        has_more: bool,
        last_common_read: Option<i64>,
    },
    /// Messages directly following the cached range; `updates` replace stored versions.
    Append {
        token: String,
        messages: Vec<ChatMessage>,
        updates: Vec<ChatMessage>,
        last_common_read: Option<i64>,
    },
    /// Messages directly below `below` (the current `oldest_id`).
    Prepend {
        token: String,
        below: i64,
        messages: Vec<ChatMessage>,
        has_more: bool,
    },
    ReadStatus { token: String, last_common_read: i64 },
    Clear,
    Flush(tokio::sync::oneshot::Sender<()>),
}

struct Inner {
    read: Mutex<Connection>,
    tx: Mutex<mpsc::Sender<Op>>,
    media: PathBuf,
}

/// Without a data directory every call is a no-op (nothing is stored).
pub(crate) struct Cache {
    inner: Option<Inner>,
}

impl Cache {
    pub fn open(dir: Option<&Path>) -> Self {
        let inner = dir.and_then(|dir| match Inner::open(dir) {
            Ok(i) => Some(i),
            Err(e) => {
                tracing::warn!("cache unavailable: {e}");
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
        let (tx, rx) = tokio::sync::oneshot::channel();
        if self.inner.is_none() {
            return;
        }
        self.send(Op::Flush(tx));
        let _ = rx.await;
    }

    fn read<T: Default>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> T {
        let Some(i) = &self.inner else { return T::default() };
        let conn = i.read.lock().unwrap();
        f(&conn).unwrap_or_else(|e| {
            tracing::warn!("cache read failed: {e}");
            T::default()
        })
    }

    // --- reads ---------------------------------------------------------------------

    pub fn meta(&self, key: &str) -> Option<String> {
        self.read(|c| {
            c.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
                .optional()
        })
    }

    pub fn rooms(&self) -> Vec<Room> {
        self.read(|c| {
            let mut st = c.prepare("SELECT json FROM rooms")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows
                .filter_map(|j| serde_json::from_str(&j.ok()?).ok())
                .collect())
        })
    }

    pub fn range(&self, token: &str) -> Option<ChatRange> {
        self.read(|c| range(c, token))
    }

    /// Up to `limit` cached messages below `before` (newest ones when `None`), ascending,
    /// and whether more cached ones exist below them.
    pub fn messages(&self, token: &str, before: Option<i64>, limit: u32) -> (Vec<ChatMessage>, bool) {
        self.read(|c| {
            let Some(r) = range(c, token)? else { return Ok((Vec::new(), false)) };
            let mut st = c.prepare(
                "SELECT json FROM messages WHERE token = ?1 AND id >= ?2 AND id < ?3
                 ORDER BY id DESC LIMIT ?4",
            )?;
            let rows = st.query_map(
                params![token, r.oldest_id, before.unwrap_or(i64::MAX), limit as i64 + 1],
                |r| r.get::<_, String>(0),
            )?;
            let mut out: Vec<ChatMessage> = rows
                .filter_map(|j| serde_json::from_str(&j.ok()?).ok())
                .collect();
            let more = out.len() > limit as usize;
            out.truncate(limit as usize);
            out.reverse();
            Ok((out, more))
        })
    }

    // --- writes --------------------------------------------------------------------

    pub fn set_meta(&self, key: &'static str, value: String) {
        self.send(Op::Meta(key, value));
    }

    pub fn put_rooms(&self, rooms: Vec<Room>, full: bool) {
        self.send(Op::Rooms { rooms, full });
    }

    pub fn replace_chat(
        &self,
        token: &str,
        messages: Vec<ChatMessage>,
        has_more: bool,
        last_common_read: Option<i64>,
    ) {
        self.send(Op::ReplaceChat {
            token: token.to_string(),
            messages,
            has_more,
            last_common_read,
        });
    }

    pub fn append(
        &self,
        token: &str,
        messages: Vec<ChatMessage>,
        updates: Vec<ChatMessage>,
        last_common_read: Option<i64>,
    ) {
        self.send(Op::Append {
            token: token.to_string(),
            messages,
            updates,
            last_common_read,
        });
    }

    pub fn prepend(&self, token: &str, below: i64, messages: Vec<ChatMessage>, has_more: bool) {
        self.send(Op::Prepend {
            token: token.to_string(),
            below,
            messages,
            has_more,
        });
    }

    pub fn set_read_status(&self, token: &str, last_common_read: i64) {
        self.send(Op::ReadStatus {
            token: token.to_string(),
            last_common_read,
        });
    }

    /// Forgets everything (sign-out).
    pub fn clear(&self) {
        self.send(Op::Clear);
        if let Some(i) = &self.inner {
            let _ = std::fs::remove_dir_all(&i.media);
        }
    }

    // --- media files ---------------------------------------------------------------

    fn media_path(&self, key: &str) -> Option<PathBuf> {
        let i = self.inner.as_ref()?;
        let hash = Sha256::digest(key.as_bytes());
        let name: String = hash[..16].iter().map(|b| format!("{b:02x}")).collect();
        Some(i.media.join(name))
    }

    /// Cached bytes and content type for `key`.
    pub async fn media_get(&self, key: &str) -> Option<(Vec<u8>, String)> {
        let path = self.media_path(key)?;
        let data = tokio::fs::read(&path).await.ok()?;
        // Recently used files survive pruning.
        let _ = std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_modified(SystemTime::now()));
        let split = data.iter().position(|&b| b == b'\n')?;
        let mime = String::from_utf8(data[..split].to_vec()).ok()?;
        Some((data[split + 1..].to_vec(), mime))
    }

    pub async fn media_put(&self, key: &str, data: &[u8], mime: &str) {
        let Some(path) = self.media_path(key) else { return };
        let tmp = path.with_extension("tmp");
        let mut buf = Vec::with_capacity(mime.len() + 1 + data.len());
        buf.extend_from_slice(mime.as_bytes());
        buf.push(b'\n');
        buf.extend_from_slice(data);
        if let Some(dir) = path.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        if tokio::fs::write(&tmp, &buf).await.is_ok() {
            let _ = tokio::fs::rename(&tmp, &path).await;
        }
    }

    /// Deletes the least recently used media files above the size limit (blocking).
    pub fn prune_media(&self) {
        if let Some(i) = &self.inner {
            prune(&i.media, MEDIA_LIMIT);
        }
    }
}

impl Inner {
    fn open(dir: &Path) -> rusqlite::Result<Self> {
        let _ = std::fs::create_dir_all(dir);
        let file = dir.join("cache.sqlite");
        let write = match open_db(&file) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("cache broken, starting empty: {e}");
                for ext in ["", "-wal", "-shm"] {
                    let _ = std::fs::remove_file(format!("{}{ext}", file.display()));
                }
                open_db(&file)?
            }
        };
        let read = Connection::open(&file)?;
        read.busy_timeout(Duration::from_secs(2))?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("talk-cache".into())
            .spawn(move || writer(write, rx))
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Ok(Self {
            read: Mutex::new(read),
            tx: Mutex::new(tx),
            media: dir.join("media"),
        })
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
            "DROP TABLE IF EXISTS meta;
             DROP TABLE IF EXISTS rooms;
             DROP TABLE IF EXISTS messages;
             DROP TABLE IF EXISTS chats;
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE rooms (token TEXT PRIMARY KEY, json TEXT NOT NULL);
             CREATE TABLE messages (
                 token TEXT NOT NULL, id INTEGER NOT NULL, json TEXT NOT NULL,
                 PRIMARY KEY (token, id)
             ) WITHOUT ROWID;
             CREATE TABLE chats (
                 token TEXT PRIMARY KEY, oldest_id INTEGER NOT NULL,
                 has_more INTEGER NOT NULL, last_common_read INTEGER
             );",
        )?;
        c.pragma_update(None, "user_version", SCHEMA)?;
    }
    // Fails loudly on a corrupt file instead of on the first real query.
    c.query_row("SELECT count(*) FROM chats", [], |_| Ok(()))?;
    Ok(c)
}

fn range(c: &Connection, token: &str) -> rusqlite::Result<Option<ChatRange>> {
    c.query_row(
        "SELECT c.oldest_id, c.has_more, c.last_common_read,
                (SELECT max(id) FROM messages m WHERE m.token = c.token)
         FROM chats c WHERE c.token = ?1",
        [token],
        |r| {
            Ok(ChatRange {
                oldest_id: r.get(0)?,
                has_more: r.get(1)?,
                last_common_read: r.get(2)?,
                newest_id: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            })
        },
    )
    .optional()
}

fn writer(mut conn: Connection, rx: mpsc::Receiver<Op>) {
    while let Ok(op) = rx.recv() {
        if let Op::Flush(done) = op {
            let _ = done.send(());
            continue;
        }
        if let Err(e) = apply(&mut conn, op) {
            tracing::warn!("cache write failed: {e}");
        }
    }
}

fn json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

fn insert_messages(tx: &rusqlite::Transaction, token: &str, messages: &[ChatMessage]) -> rusqlite::Result<()> {
    let mut st = tx.prepare_cached("INSERT OR REPLACE INTO messages (token, id, json) VALUES (?1, ?2, ?3)")?;
    for m in messages {
        st.execute(params![token, m.id, json(m)])?;
    }
    Ok(())
}

fn delete_chat(tx: &rusqlite::Transaction, token: &str) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM messages WHERE token = ?1", [token])?;
    tx.execute("DELETE FROM chats WHERE token = ?1", [token])?;
    Ok(())
}

/// Keeps the newest [`MAX_PER_ROOM`] messages of a chat.
fn trim(tx: &rusqlite::Transaction, token: &str) -> rusqlite::Result<()> {
    let cutoff: Option<i64> = tx
        .query_row(
            "SELECT id FROM messages WHERE token = ?1 ORDER BY id DESC LIMIT 1 OFFSET ?2",
            params![token, MAX_PER_ROOM - 1],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(cutoff) = cutoff {
        let n = tx.execute("DELETE FROM messages WHERE token = ?1 AND id < ?2", params![token, cutoff])?;
        if n > 0 {
            tx.execute(
                "UPDATE chats SET oldest_id = ?2, has_more = 1 WHERE token = ?1",
                params![token, cutoff],
            )?;
        }
    }
    Ok(())
}

fn apply(conn: &mut Connection, op: Op) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    match op {
        Op::Flush(_) => {}
        Op::Meta(key, value) => {
            tx.execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", params![key, value])?;
        }
        Op::Rooms { rooms, full } => {
            if full {
                let keep: std::collections::HashSet<&str> = rooms.iter().map(|r| r.token.as_str()).collect();
                let gone: Vec<String> = {
                    let mut st = tx.prepare("SELECT token FROM rooms")?;
                    st.query_map([], |r| r.get::<_, String>(0))?
                        .filter_map(|t| t.ok())
                        .filter(|t| !keep.contains(t.as_str()))
                        .collect()
                };
                for t in gone {
                    tx.execute("DELETE FROM rooms WHERE token = ?1", [&t])?;
                    delete_chat(&tx, &t)?;
                }
            }
            let mut st = tx.prepare_cached("INSERT OR REPLACE INTO rooms (token, json) VALUES (?1, ?2)")?;
            for r in &rooms {
                st.execute(params![r.token, json(r)])?;
            }
        }
        Op::ReplaceChat {
            token,
            messages,
            has_more,
            last_common_read,
        } => {
            delete_chat(&tx, &token)?;
            if let Some(oldest) = messages.iter().map(|m| m.id).min() {
                insert_messages(&tx, &token, &messages)?;
                tx.execute(
                    "INSERT INTO chats (token, oldest_id, has_more, last_common_read) VALUES (?1, ?2, ?3, ?4)",
                    params![token, oldest, has_more, last_common_read],
                )?;
                trim(&tx, &token)?;
            }
        }
        Op::Append {
            token,
            messages,
            updates,
            last_common_read,
        } => {
            // Without a cached range the messages would start a range with a gap.
            if range(&tx, &token)?.is_some() {
                insert_messages(&tx, &token, &messages)?;
                let mut st = tx.prepare_cached("UPDATE messages SET json = ?3 WHERE token = ?1 AND id = ?2")?;
                for m in &updates {
                    st.execute(params![token, m.id, json(m)])?;
                }
                drop(st);
                if let Some(r) = last_common_read {
                    tx.execute("UPDATE chats SET last_common_read = ?2 WHERE token = ?1", params![token, r])?;
                }
                trim(&tx, &token)?;
            }
        }
        Op::Prepend {
            token,
            below,
            messages,
            has_more,
        } => {
            let count: i64 = tx.query_row("SELECT count(*) FROM messages WHERE token = ?1", [&token], |r| r.get(0))?;
            let fits = count + messages.len() as i64 <= MAX_PER_ROOM;
            if fits && range(&tx, &token)?.is_some_and(|r| r.oldest_id == below) {
                insert_messages(&tx, &token, &messages)?;
                let oldest = messages.iter().map(|m| m.id).min().unwrap_or(below);
                tx.execute(
                    "UPDATE chats SET oldest_id = ?2, has_more = ?3 WHERE token = ?1",
                    params![token, oldest, has_more],
                )?;
            }
        }
        Op::ReadStatus { token, last_common_read } => {
            tx.execute(
                "UPDATE chats SET last_common_read = ?2 WHERE token = ?1",
                params![token, last_common_read],
            )?;
        }
        Op::Clear => {
            tx.execute_batch("DELETE FROM meta; DELETE FROM rooms; DELETE FROM messages; DELETE FROM chats;")?;
        }
    }
    tx.commit()
}

pub(crate) fn prune(dir: &Path, limit: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let m = e.metadata().ok()?;
            m.is_file()
                .then(|| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= limit {
        return;
    }
    files.sort_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= limit * 8 / 10 {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(token: &str, id: i64, text: &str) -> ChatMessage {
        serde_json::from_value(serde_json::json!({
            "id": id, "token": token, "actorType": "users", "actorId": "a",
            "actorDisplayName": "A", "timestamp": id, "message": text,
        }))
        .unwrap()
    }

    fn room(token: &str) -> Room {
        serde_json::from_value(serde_json::json!({
            "id": 1, "token": token, "type": 2, "name": token, "displayName": token,
            "lastMessage": { "id": 9, "token": token, "actorType": "users", "actorId": "a",
                "actorDisplayName": "A", "timestamp": 9, "message": "hi",
                "messageParameters": [] },
        }))
        .unwrap()
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("talk-cache-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[tokio::test]
    async fn rooms_roundtrip_and_full_sync_drops_left_rooms() {
        let d = dir("rooms");
        let c = Cache::open(Some(&d));
        c.put_rooms(vec![room("a"), room("b")], true);
        c.replace_chat("b", vec![msg("b", 1, "x")], false, None);
        c.flush().await;
        assert_eq!(c.rooms().len(), 2);
        assert_eq!(c.rooms()[0].last_message.as_ref().unwrap().message, "hi");

        c.put_rooms(vec![room("a")], true);
        c.flush().await;
        assert_eq!(c.rooms().len(), 1);
        assert!(c.range("b").is_none());
        assert!(c.messages("b", None, 50).0.is_empty());

        // Survives reopening.
        drop(c);
        let c = Cache::open(Some(&d));
        assert_eq!(c.rooms()[0].token, "a");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn range_append_prepend_and_updates() {
        let d = dir("range");
        let c = Cache::open(Some(&d));
        // Append without a range is ignored (it would leave a gap).
        c.append("t", vec![msg("t", 5, "lost")], vec![], None);
        c.replace_chat("t", (10..20).map(|i| msg("t", i, "m")).collect(), true, Some(12));
        c.append("t", vec![msg("t", 20, "new")], vec![msg("t", 11, "edited")], Some(15));
        c.flush().await;
        let r = c.range("t").unwrap();
        assert_eq!((r.oldest_id, r.newest_id, r.has_more, r.last_common_read), (10, 20, true, Some(15)));
        let (page, more) = c.messages("t", None, 5);
        assert_eq!(page.iter().map(|m| m.id).collect::<Vec<_>>(), vec![16, 17, 18, 19, 20]);
        assert!(more);
        let (older, more) = c.messages("t", Some(12), 5);
        assert_eq!(older.iter().map(|m| m.id).collect::<Vec<_>>(), vec![10, 11]);
        assert!(!more);
        assert_eq!(older[1].message, "edited");

        // Prepend only directly below the range.
        c.prepend("t", 7, vec![msg("t", 3, "x")], true);
        c.prepend("t", 10, vec![msg("t", 8, "a"), msg("t", 9, "b")], false);
        c.flush().await;
        let r = c.range("t").unwrap();
        assert_eq!((r.oldest_id, r.has_more), (8, false));
        assert_eq!(c.messages("t", Some(10), 50).0.len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn trims_to_newest() {
        let d = dir("trim");
        let c = Cache::open(Some(&d));
        c.replace_chat("t", (1..=10).map(|i| msg("t", i, "m")).collect(), false, None);
        let many: Vec<_> = (11..=MAX_PER_ROOM + 5).map(|i| msg("t", i, "m")).collect();
        c.append("t", many, vec![], None);
        c.flush().await;
        let r = c.range("t").unwrap();
        assert_eq!(r.newest_id, MAX_PER_ROOM + 5);
        assert_eq!(r.oldest_id, 6);
        assert!(r.has_more);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn schema_change_starts_empty_and_clear_forgets() {
        let d = dir("schema");
        let c = Cache::open(Some(&d));
        c.set_meta("user_id", "me".into());
        c.put_rooms(vec![room("a")], true);
        c.flush().await;
        assert_eq!(c.meta("user_id").as_deref(), Some("me"));
        drop(c);
        {
            let conn = Connection::open(d.join("cache.sqlite")).unwrap();
            conn.pragma_update(None, "user_version", 0).unwrap();
        }
        let c = Cache::open(Some(&d));
        assert!(c.rooms().is_empty());
        assert!(c.meta("user_id").is_none());

        c.put_rooms(vec![room("a")], true);
        c.media_put("k", b"png-bytes", "image/png").await;
        assert_eq!(c.media_get("k").await, Some((b"png-bytes".to_vec(), "image/png".into())));
        c.clear();
        c.flush().await;
        assert!(c.rooms().is_empty());
        assert!(c.media_get("k").await.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn disabled_without_dir() {
        let c = Cache::open(None);
        c.put_rooms(vec![room("a")], true);
        c.flush().await;
        assert!(!c.enabled());
        assert!(c.rooms().is_empty());
    }

    #[test]
    fn prune_keeps_recent() {
        let d = dir("prune");
        std::fs::create_dir_all(&d).unwrap();
        for i in 0..10 {
            let p = d.join(format!("f{i}"));
            std::fs::write(&p, vec![0u8; 100]).unwrap();
            let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + i);
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
        }
        prune(&d, 500);
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).collect();
        assert_eq!(left.len(), 4);
        assert!(d.join("f9").exists() && !d.join("f0").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
