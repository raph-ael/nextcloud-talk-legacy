//! Registered devices (SQLite).

use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone)]
pub struct Store(Arc<Mutex<Connection>>);

pub struct Device {
    pub id: String,
    pub push_token: String,
    pub push_token_hash: String,
    pub user_public_key: String,
}

impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let c = Connection::open(path)?;
        c.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS devices (
                id TEXT PRIMARY KEY,
                push_token TEXT NOT NULL,
                push_token_hash TEXT NOT NULL,
                user_public_key TEXT NOT NULL,
                apns_sandbox INTEGER,
                updated INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );",
        )?;
        Ok(Self(Arc::new(Mutex::new(c))))
    }

    pub fn put(&self, d: &Device) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO devices (id, push_token, push_token_hash, user_public_key, apns_sandbox)
             VALUES (?1, ?2, ?3, ?4, NULL)
             ON CONFLICT(id) DO UPDATE SET push_token = ?2, push_token_hash = ?3,
               user_public_key = ?4, apns_sandbox = NULL, updated = strftime('%s','now')",
            params![d.id, d.push_token, d.push_token_hash, d.user_public_key],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> rusqlite::Result<Option<Device>> {
        self.0
            .lock()
            .unwrap()
            .query_row(
                "SELECT id, push_token, push_token_hash, user_public_key FROM devices WHERE id = ?1",
                [id],
                |r| {
                    Ok(Device {
                        id: r.get(0)?,
                        push_token: r.get(1)?,
                        push_token_hash: r.get(2)?,
                        user_public_key: r.get(3)?,
                    })
                },
            )
            .optional()
    }

    pub fn delete(&self, id: &str) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute("DELETE FROM devices WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Development builds use the APNs sandbox; remembered after the first try.
    pub fn apns_sandbox(&self, id: &str) -> Option<bool> {
        self.0
            .lock()
            .unwrap()
            .query_row("SELECT apns_sandbox FROM devices WHERE id = ?1", [id], |r| r.get::<_, Option<bool>>(0))
            .ok()
            .flatten()
    }

    pub fn set_apns_sandbox(&self, id: &str, sandbox: bool) {
        let _ = self
            .0
            .lock()
            .unwrap()
            .execute("UPDATE devices SET apns_sandbox = ?2 WHERE id = ?1", params![id, sandbox]);
    }
}
