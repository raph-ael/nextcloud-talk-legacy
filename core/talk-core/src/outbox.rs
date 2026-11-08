//! Persistent outbox: everything the user sends goes through here and is delivered
//! exactly once, surviving network loss, app restarts and reboots.
//!
//! Design (modelled on Signal's job system, adapted to Talk):
//! - Every outgoing message is an [`OutboxItem`] stored as one JSON file under
//!   `{data_dir}/outbox/`, rewritten atomically on every step. Attachments are moved into
//!   `{data_dir}/outbox/files/` first, so the source may disappear.
//! - One task per item: upload (max. [`PARALLEL_UPLOADS`] at once), then post. Posting is
//!   strictly in send order per conversation – Talk orders by server id, so a later text
//!   must not overtake an earlier photo.
//! - Retries: network errors, 5xx, 408 and 429 back off exponentially (2^n s, at most
//!   60 s, ±25 % jitter, `Retry-After` wins) and wake immediately when the network
//!   returns. After [`LIFESPAN`] an item fails; permanent errors fail at once.
//! - Exactly once: before repeating a step whose request may have reached the server,
//!   the outbox checks whether it took effect (uploaded file with the same size, existing
//!   share, message with the same reference id). Large files use Nextcloud's resumable
//!   chunked upload, so an interrupted upload continues where it stopped.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, Semaphore};
use tokio::task::AbortHandle;

use crate::client::{PutOutcome, remote_path};
use crate::engine::{Engine, Event, MessageView, ReplyPreview};
use crate::error::{Error, Result};
use crate::models::FileAttachment;

/// How long an item is retried before it is marked failed (Signal: one day).
pub const LIFESPAN: Duration = Duration::from_secs(24 * 3600);
const MAX_BACKOFF_S: u64 = 60;
/// Files above this size use the resumable chunked upload, in chunks of this size.
pub const CHUNK_SIZE: u64 = 10 * 1024 * 1024;
const PARALLEL_UPLOADS: usize = 3;
/// A posted file stays visible as pending until its echo arrives, at most this long.
const ECHO_GRACE: Duration = Duration::from_secs(30);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Why an item finally failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SendError {
    /// Not delivered within [`LIFESPAN`] (offline for too long).
    Expired,
    /// No permission (read-only conversation, removed from it, sharing disabled).
    Forbidden,
    /// The conversation no longer exists.
    NotFound,
    TooLarge,
    QuotaExceeded,
    /// The local copy of the attachment is gone.
    FileMissing,
    /// Any other refusal by the server.
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutFile {
    /// Local copy inside the outbox (or the caller's path without a data dir).
    pub local: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub width: i64,
    pub height: i64,
    /// Target path being written; checked after an unclear failure.
    #[serde(default)]
    pub candidate: Option<String>,
    /// Number of the name variant (`name (n).ext`) of `candidate`.
    #[serde(default = "one")]
    pub name_n: u32,
    /// Chunked upload session id.
    #[serde(default)]
    pub upload_id: Option<String>,
    /// Final path once the upload is complete.
    #[serde(default)]
    pub remote: Option<String>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxItem {
    pub reference_id: String,
    pub token: String,
    /// Global send order.
    pub seq: u64,
    /// Unix seconds.
    pub created_at: i64,
    pub expires_at: i64,
    /// Message text, or the caption of a file.
    pub text: String,
    pub reply_to: Option<ReplyPreview>,
    pub file: Option<OutFile>,
    /// A post request was started and its outcome is unknown: check before repeating.
    #[serde(default)]
    pub post_attempted: bool,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub failed: Option<SendError>,
}

/// What the UI shows for a pending message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PendingState {
    Sending,
    /// Waiting for the network (or the server) to come back; retried automatically.
    Waiting,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingMessage {
    pub message: MessageView,
    pub state: PendingState,
    /// Upload progress 0..1, negative when not uploading.
    pub progress: f32,
    pub error: Option<SendError>,
    /// Local copy of the attachment (for showing it before the server has it).
    pub local_path: Option<String>,
}

/// Result of [`Engine::wait_outbox_idle`].
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxIdle {
    /// Items waiting for the network or the server.
    pub waiting: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Queued,
    Uploading,
    Sending,
    Waiting,
    /// Delivered; kept until the echo arrives so the bubble does not blink.
    Sent,
}

struct Live {
    phase: Phase,
    sent_bytes: u64,
    last_emit: Option<Instant>,
    task: Option<AbortHandle>,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            phase: Phase::Queued,
            sent_bytes: 0,
            last_emit: None,
            task: None,
        }
    }
}

#[derive(Default)]
struct Inner {
    items: BTreeMap<String, OutboxItem>,
    live: HashMap<String, Live>,
}

pub(crate) struct Outbox {
    store: Store,
    inner: Mutex<Inner>,
    /// Any state change (ordering and idle waits).
    changed: Notify,
    /// Cuts backoffs short: network back, manual retry.
    wake: Notify,
    uploads: Semaphore,
    seq: AtomicU64,
}

impl Outbox {
    pub(crate) fn open(data_dir: Option<&Path>) -> Self {
        let store = Store {
            dir: data_dir.map(|d| d.join("outbox")),
        };
        let items = store.load();
        let seq = items.iter().map(|i| i.seq).max().unwrap_or(0);
        Self {
            store,
            inner: Mutex::new(Inner {
                live: items
                    .iter()
                    .map(|i| (i.reference_id.clone(), Live::default()))
                    .collect(),
                items: items.into_iter().map(|i| (i.reference_id.clone(), i)).collect(),
            }),
            changed: Notify::new(),
            wake: Notify::new(),
            uploads: Semaphore::new(PARALLEL_UPLOADS),
            seq: AtomicU64::new(seq),
        }
    }

    fn get(&self, r: &str) -> Option<OutboxItem> {
        self.inner.lock().unwrap().items.get(r).cloned()
    }

    /// Changes an item in memory and on disk. No-op for removed items.
    fn update(&self, r: &str, f: impl FnOnce(&mut OutboxItem)) -> Option<OutboxItem> {
        let item = {
            let mut inner = self.inner.lock().unwrap();
            let item = inner.items.get_mut(r)?;
            f(item);
            item.clone()
        };
        self.store.save(&item);
        Some(item)
    }

    fn phase(&self, r: &str) -> Option<Phase> {
        self.inner.lock().unwrap().live.get(r).map(|l| l.phase)
    }

    /// Wakes every task sleeping in a backoff.
    pub(crate) fn wake(&self) {
        self.wake.notify_waiters();
    }

    /// Removes everything (sign-out).
    pub(crate) fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        for l in inner.live.values_mut() {
            if let Some(t) = l.task.take() {
                t.abort();
            }
        }
        inner.items.clear();
        inner.live.clear();
        self.store.clear();
    }

    pub(crate) fn abort_all(&self) {
        let mut inner = self.inner.lock().unwrap();
        for l in inner.live.values_mut() {
            if let Some(t) = l.task.take() {
                t.abort();
            }
        }
    }
}

// --- engine side ---------------------------------------------------------------------

impl Engine {
    /// Queues a text message. It shows up as pending at once (`OutboxChanged`) and is
    /// delivered in the background; the returned value is the pending bubble.
    pub fn enqueue_text(
        self: &Arc<Self>,
        token: &str,
        text: &str,
        reply_to: Option<ReplyPreview>,
        reference_id: &str,
    ) -> PendingMessage {
        self.enqueue(token, text, reply_to, None, reference_id)
    }

    /// Queues a file (with optional caption). The file at `local_path` is moved into the
    /// outbox (copied when it is on another file system); the caller must not reuse it.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_file(
        self: &Arc<Self>,
        token: &str,
        local_path: &str,
        name: &str,
        mime: &str,
        width: i64,
        height: i64,
        caption: &str,
        reply_to: Option<ReplyPreview>,
        reference_id: &str,
    ) -> Result<PendingMessage> {
        let local = self
            .outbox
            .store
            .adopt_file(reference_id, Path::new(local_path), name)
            .map_err(|e| Error::LocalFile(format!("{local_path}: {e}")))?;
        let size = std::fs::metadata(&local)
            .map_err(|e| Error::LocalFile(format!("{}: {e}", local.display())))?
            .len();
        let file = OutFile {
            local: local.to_string_lossy().into_owned(),
            name: sanitize_name(name),
            mime: mime.to_string(),
            size,
            width,
            height,
            candidate: None,
            name_n: 1,
            upload_id: None,
            remote: None,
        };
        Ok(self.enqueue(token, caption.trim(), reply_to, Some(file), reference_id))
    }

    fn enqueue(
        self: &Arc<Self>,
        token: &str,
        text: &str,
        reply_to: Option<ReplyPreview>,
        file: Option<OutFile>,
        reference_id: &str,
    ) -> PendingMessage {
        let now = unix_now();
        let item = OutboxItem {
            reference_id: reference_id.to_string(),
            token: token.to_string(),
            seq: self.outbox.seq.fetch_add(1, Ordering::Relaxed) + 1,
            created_at: now,
            expires_at: now + LIFESPAN.as_secs() as i64,
            text: text.to_string(),
            reply_to,
            file,
            post_attempted: false,
            attempts: 0,
            failed: None,
        };
        self.outbox.store.save(&item);
        {
            let mut inner = self.outbox.inner.lock().unwrap();
            inner.items.insert(item.reference_id.clone(), item.clone());
            inner.live.insert(item.reference_id.clone(), Live::default());
        }
        self.spawn_item(&item.reference_id);
        self.outbox_changed(token);
        self.pending_view(&item, &Live::default())
    }

    /// Pending (unsent or failed) messages of a conversation, in send order.
    pub fn pending(&self, token: &str) -> Vec<PendingMessage> {
        let inner = self.outbox.inner.lock().unwrap();
        let mut list: Vec<&OutboxItem> = inner.items.values().filter(|i| i.token == token).collect();
        list.sort_by_key(|i| i.seq);
        list.into_iter()
            .map(|i| {
                let live = inner.live.get(&i.reference_id);
                self.pending_view(i, live.unwrap_or(&Live::default()))
            })
            .collect()
    }

    /// Number of items still to deliver (not failed).
    pub fn outbox_unsent(&self) -> u32 {
        let inner = self.outbox.inner.lock().unwrap();
        inner
            .items
            .iter()
            .filter(|(r, i)| {
                i.failed.is_none() && inner.live.get(*r).map(|l| l.phase) != Some(Phase::Sent)
            })
            .count() as u32
    }

    /// Bytes of attachments still to upload (the platform keeps the app in the
    /// foreground above a threshold).
    pub fn outbox_upload_bytes(&self) -> u64 {
        let inner = self.outbox.inner.lock().unwrap();
        inner
            .items
            .values()
            .filter(|i| i.failed.is_none())
            .filter_map(|i| i.file.as_ref())
            .filter(|f| f.remote.is_none())
            .map(|f| f.size)
            .sum()
    }

    /// Sends a failed item again (with a fresh lifespan) or retries a waiting one now.
    pub fn retry_send(self: &Arc<Self>, reference_id: &str) {
        let Some(item) = self.outbox.update(reference_id, |i| {
            if i.failed.is_some() {
                i.failed = None;
                i.attempts = 0;
                i.expires_at = unix_now() + LIFESPAN.as_secs() as i64;
            }
        }) else {
            return;
        };
        self.set_phase(reference_id, Phase::Queued);
        self.spawn_item(reference_id);
        self.outbox.wake();
        self.outbox_changed(&item.token);
    }

    /// Drops a pending or failed item (and its local copy).
    pub fn discard(self: &Arc<Self>, reference_id: &str) {
        let Some(item) = self.remove_item(reference_id) else { return };
        if let Some(id) = item.file.as_ref().and_then(|f| f.upload_id.clone()) {
            let engine = self.clone();
            self.spawn(async move {
                let _ = engine.client.chunk_abort(&engine.user_id(), &id).await;
            });
        }
        self.outbox_changed(&item.token);
    }

    /// The platform saw the network come back (or change): retry now.
    pub fn network_changed(&self) {
        self.outbox.wake();
    }

    /// Resolves once nothing is actively being sent: the outbox is empty or every item is
    /// failed or waiting for the network. Used by background workers.
    pub async fn wait_outbox_idle(&self) -> OutboxIdle {
        loop {
            let notified = self.outbox.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let mut idle = OutboxIdle::default();
            let mut active = 0;
            {
                let inner = self.outbox.inner.lock().unwrap();
                for (r, item) in &inner.items {
                    let phase = inner.live.get(r).map(|l| l.phase).unwrap_or(Phase::Queued);
                    if item.failed.is_some() {
                        idle.failed += 1;
                    } else if phase == Phase::Waiting {
                        idle.waiting += 1;
                    } else if phase != Phase::Sent {
                        active += 1;
                    }
                }
            }
            if active == 0 {
                return idle;
            }
            notified.await;
        }
    }

    // --- internals -------------------------------------------------------------------

    /// Starts the tasks of all loaded items (after a restart).
    pub(crate) fn spawn_outbox(self: &Arc<Self>) {
        let refs: Vec<String> = self.outbox.inner.lock().unwrap().items.keys().cloned().collect();
        for r in refs {
            self.spawn_item(&r);
        }
    }

    fn spawn_item(self: &Arc<Self>, r: &str) {
        let Some(rt) = self.runtime.get() else { return }; // started later by `start`
        let mut inner = self.outbox.inner.lock().unwrap();
        let Some(item) = inner.items.get(r) else { return };
        if item.failed.is_some() {
            return;
        }
        let live = inner.live.entry(r.to_string()).or_default();
        if live.task.as_ref().is_some_and(|t| !t.is_finished()) || live.phase == Phase::Sent {
            return;
        }
        let engine = self.clone();
        let reference = r.to_string();
        live.task = Some(rt.spawn(async move { engine.run_item(reference).await }).abort_handle());
    }

    fn spawn(&self, fut: impl Future<Output = ()> + Send + 'static) {
        if let Some(rt) = self.runtime.get() {
            rt.spawn(fut);
        }
    }

    async fn run_item(self: Arc<Self>, r: String) {
        loop {
            let wake = self.outbox.wake.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();

            let Some(item) = self.outbox.get(&r) else { return };
            if item.failed.is_some() || self.outbox.phase(&r) == Some(Phase::Sent) {
                return;
            }
            if unix_now() > item.expires_at {
                self.fail(&r, SendError::Expired);
                return;
            }
            let result = self.attempt(&r).await;
            if self.outbox.get(&r).is_none() {
                return; // discarded or completed by its echo meanwhile
            }
            let err = match result {
                Ok(()) => return,
                Err(e) => e,
            };
            tracing::warn!(reference = %r, "send attempt failed: {err}");
            match classify(&err) {
                Verdict::Fail(kind) => {
                    self.fail(&r, kind);
                    return;
                }
                Verdict::Pause => {
                    // Credentials rejected: the platform signs out (AuthFailed) or the
                    // next successful request wakes us.
                    self.set_phase(&r, Phase::Waiting);
                    self.outbox_changed(&item.token);
                    wake.await;
                }
                Verdict::Retry { after, network } => {
                    let attempts = self
                        .outbox
                        .update(&r, |i| {
                            i.attempts += 1;
                            if !err.maybe_delivered() {
                                i.post_attempted = false;
                            }
                        })
                        .map(|i| i.attempts)
                        .unwrap_or(1);
                    self.set_phase(&r, if network { Phase::Waiting } else { Phase::Queued });
                    self.outbox_changed(&item.token);
                    let delay = after.unwrap_or_else(|| backoff(attempts, jitter()));
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = &mut wake => {}
                    }
                    self.set_phase(&r, Phase::Queued);
                }
            }
        }
    }

    /// One pass over the remaining steps of an item.
    async fn attempt(self: &Arc<Self>, r: &str) -> Result<()> {
        self.ensure_user().await?;
        let item = self.outbox.get(r).ok_or(Error::NotFound)?;
        if item.file.as_ref().is_some_and(|f| f.remote.is_none()) {
            let _slot = self.outbox.uploads.acquire().await.expect("semaphore closed");
            self.set_phase(r, Phase::Uploading);
            self.outbox_changed(&item.token);
            self.upload_item(r).await?;
        }
        self.wait_turn(r).await;
        let Some(item) = self.outbox.get(r) else { return Ok(()) };
        self.set_phase(r, Phase::Sending);
        self.outbox_changed(&item.token);

        if item.post_attempted && self.already_posted(&item).await? {
            tracing::info!(reference = %r, "already delivered, not sending again");
            self.delivered(r, None);
            return Ok(());
        }
        self.outbox.update(r, |i| i.post_attempted = true);
        let reply = item.reply_to.as_ref().map(|p| p.id).filter(|id| *id > 0);
        match &item.file {
            Some(f) => {
                let remote = f.remote.as_deref().unwrap_or_default();
                let caption = Some(item.text.as_str()).filter(|t| !t.is_empty());
                self.guard(self.client.share_to_room(&item.token, remote, r, caption, reply).await)?;
                self.delivered(r, None);
            }
            None => {
                let msg = self.guard(self.client.send_message(&item.token, &item.text, r, reply).await)?;
                self.delivered(r, Some(msg));
            }
        }
        Ok(())
    }

    /// Waits until every earlier unsent item of the same conversation is delivered (or
    /// failed): messages arrive in the order they were sent.
    async fn wait_turn(&self, r: &str) {
        loop {
            let notified = self.outbox.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let inner = self.outbox.inner.lock().unwrap();
                let Some(item) = inner.items.get(r) else { return };
                let blocked = inner.items.values().any(|o| {
                    o.token == item.token
                        && o.seq < item.seq
                        && o.failed.is_none()
                        && inner.live.get(&o.reference_id).map(|l| l.phase) != Some(Phase::Sent)
                });
                if !blocked {
                    return;
                }
            }
            notified.await;
        }
    }

    async fn already_posted(&self, item: &OutboxItem) -> Result<bool> {
        match &item.file {
            Some(f) => {
                let remote = f.remote.as_deref().unwrap_or_default();
                self.guard(self.client.is_shared_to_room(remote, &item.token).await)
            }
            None => {
                let batch = self.guard(self.client.history(&item.token, None, 50).await)?;
                Ok(batch.messages.iter().any(|m| m.reference_id == item.reference_id))
            }
        }
    }

    async fn upload_item(self: &Arc<Self>, r: &str) -> Result<()> {
        let folder = self.attachment_folder().await?;
        let item = self.outbox.get(r).ok_or(Error::NotFound)?;
        let f = item.file.clone().ok_or(Error::NotFound)?;
        if !Path::new(&f.local).is_file() {
            return Err(Error::LocalFile(format!("{} missing", f.local)));
        }
        let remote = if f.size > CHUNK_SIZE {
            self.upload_chunked(r, &folder, &f).await?
        } else {
            self.upload_simple(r, &folder, &f).await?
        };
        self.outbox.update(r, |i| {
            if let Some(file) = i.file.as_mut() {
                file.remote = Some(remote.clone());
                file.candidate = None;
                file.upload_id = None;
            }
        });
        Ok(())
    }

    async fn upload_simple(self: &Arc<Self>, r: &str, folder: &str, f: &OutFile) -> Result<String> {
        let user = self.user_id();
        let mut n = f.name_n;
        let mut made_folder = false;
        loop {
            let target = remote_path(folder, &f.name, n);
            if f.candidate.as_deref() == Some(target.as_str()) {
                // An earlier attempt may have completed without us hearing back.
                match self.guard(self.client.stat(&user, &target).await)? {
                    Some(size) if size == f.size => return Ok(target),
                    Some(_) => {
                        n += 1;
                        continue;
                    }
                    None => {}
                }
            }
            self.outbox.update(r, |i| {
                if let Some(file) = i.file.as_mut() {
                    file.candidate = Some(target.clone());
                    file.name_n = n;
                }
            });
            self.set_progress(r, 0);
            let progress = self.progress_callback(r);
            match self.guard(self.client.put_file(&user, &target, &f.local, &f.mime, Some(progress)).await)? {
                PutOutcome::Created => return Ok(target),
                PutOutcome::Exists if n < 1000 => n += 1,
                PutOutcome::ParentMissing if !made_folder => {
                    self.guard(self.client.mkcol_all(&user, folder).await)?;
                    made_folder = true;
                }
                other => return Err(Error::Decode(format!("upload of {target}: {other:?}"))),
            }
        }
    }

    async fn upload_chunked(self: &Arc<Self>, r: &str, folder: &str, f: &OutFile) -> Result<String> {
        let user = self.user_id();
        // Resume the session of an earlier attempt if the server still has it.
        let mut session = None;
        if let (Some(id), Some(target)) = (f.upload_id.clone(), f.candidate.clone()) {
            match self.guard(self.client.chunk_list(&user, &id).await)? {
                Some(chunks) => session = Some((id, target, chunks)),
                None => {
                    // Gone: either assembled by an earlier attempt or cleaned up.
                    if self.guard(self.client.stat(&user, &target).await)? == Some(f.size) {
                        return Ok(target);
                    }
                }
            }
        }
        let (id, mut target, chunks) = match session {
            Some(s) => s,
            None => {
                self.guard(self.client.mkcol_all(&user, folder).await)?;
                let (target, n) = self.free_name(&user, folder, &f.name, f.name_n).await?;
                let id = f
                    .upload_id
                    .clone()
                    .unwrap_or_else(|| format!("sirplus-{}", &r[..r.len().min(24)]));
                self.outbox.update(r, |i| {
                    if let Some(file) = i.file.as_mut() {
                        file.candidate = Some(target.clone());
                        file.name_n = n;
                        file.upload_id = Some(id.clone());
                    }
                });
                self.guard(self.client.chunk_begin(&user, &id, &target).await)?;
                (id, target, Vec::new())
            }
        };

        let count = f.size.div_ceil(CHUNK_SIZE).max(1);
        let done: u64 = (0..count)
            .filter(|i| chunks.contains(&((i + 1) as u32, chunk_len(f.size, *i))))
            .map(|i| chunk_len(f.size, i))
            .sum();
        if done > 0 {
            tracing::info!(reference = %r, "resuming upload, {done} of {} bytes on the server", f.size);
        }
        let mut confirmed = done;
        for i in 0..count {
            let n = (i + 1) as u32;
            let len = chunk_len(f.size, i);
            if chunks.contains(&(n, len)) {
                continue;
            }
            self.set_progress(r, confirmed);
            let progress = self.progress_callback(r);
            let res = self
                .client
                .chunk_put(&user, &id, n, &f.local, i * CHUNK_SIZE, len, &target, f.size, Some(progress))
                .await;
            match self.guard(res) {
                Ok(()) => confirmed += len,
                Err(Error::NotFound) => return Err(self.restart_chunked(r)),
                Err(e) => return Err(e),
            }
        }
        let mut n = self.outbox.get(r).and_then(|i| i.file.map(|f| f.name_n)).unwrap_or(1);
        loop {
            match self.guard(self.client.chunk_finish(&user, &id, &target, f.size).await) {
                Ok(PutOutcome::Created) => return Ok(target),
                Ok(PutOutcome::Exists) if n < 1000 => {
                    // Someone took the name meanwhile: assemble under the next free one.
                    let (t, m) = self.free_name(&user, folder, &f.name, n + 1).await?;
                    target = t;
                    n = m;
                    self.outbox.update(r, |i| {
                        if let Some(file) = i.file.as_mut() {
                            file.candidate = Some(target.clone());
                            file.name_n = n;
                        }
                    });
                }
                Ok(other) => return Err(Error::Decode(format!("assembling {target}: {other:?}"))),
                Err(Error::NotFound) => return Err(self.restart_chunked(r)),
                Err(e) => return Err(e),
            }
        }
    }

    /// The upload session vanished on the server: start over with a fresh one.
    fn restart_chunked(&self, r: &str) -> Error {
        self.outbox.update(r, |i| {
            if let Some(file) = i.file.as_mut() {
                file.upload_id = None;
                file.candidate = None;
            }
        });
        Error::Decode("upload session vanished on the server, restarting".into())
    }

    /// First `name (n)` variant from `start` on that does not exist yet.
    async fn free_name(&self, user: &str, folder: &str, name: &str, start: u32) -> Result<(String, u32)> {
        let mut n = start.max(1);
        loop {
            let target = remote_path(folder, name, n);
            if self.guard(self.client.stat(user, &target).await)?.is_none() || n >= 1000 {
                return Ok((target, n));
            }
            n += 1;
        }
    }

    /// Delivered: emits the server message for texts, keeps files visible until their
    /// echo arrives (only while the chat is being polled).
    fn delivered(&self, r: &str, message: Option<crate::models::ChatMessage>) {
        let Some(item) = self.outbox.get(r) else { return };
        if let Some(msg) = message {
            self.note_own_message(&item.token, msg);
        }
        self.outbox.store.remove_record(r);
        let chat_open = self.chat_open(&item.token);
        if item.file.is_some() && chat_open {
            self.set_phase(r, Phase::Sent);
            self.outbox_changed(&item.token);
            let engine = self.weak_self();
            let reference = r.to_string();
            self.spawn(async move {
                tokio::time::sleep(ECHO_GRACE).await;
                if let Some(e) = engine.upgrade() {
                    if let Some(item) = e.remove_item(&reference) {
                        e.outbox_changed(&item.token);
                    }
                }
            });
        } else {
            self.remove_item(r);
            self.outbox_changed(&item.token);
        }
    }

    /// Called with every live message: an echo completes its pending item.
    pub(crate) fn outbox_echo(&self, token: &str, reference_ids: &[&str]) {
        let mut changed = false;
        for r in reference_ids {
            if r.is_empty() {
                continue;
            }
            if self.remove_item(r).is_some() {
                changed = true;
            }
        }
        if changed {
            self.outbox_changed(token);
        }
    }

    fn remove_item(&self, r: &str) -> Option<OutboxItem> {
        let item = {
            let mut inner = self.outbox.inner.lock().unwrap();
            if let Some(t) = inner.live.remove(r).and_then(|l| l.task) {
                // Never abort ourselves: a task removing its own item just returns.
                if tokio::task::try_id() != Some(t.id()) {
                    t.abort();
                }
            }
            inner.items.remove(r)
        }?;
        self.outbox.store.remove(&item);
        self.outbox.changed.notify_waiters();
        Some(item)
    }

    fn fail(&self, r: &str, kind: SendError) {
        tracing::warn!(reference = %r, "send failed permanently: {kind:?}");
        if let Some(item) = self.outbox.update(r, |i| i.failed = Some(kind)) {
            self.set_phase(r, Phase::Queued);
            self.outbox_changed(&item.token);
        }
    }

    fn set_phase(&self, r: &str, phase: Phase) {
        if let Some(l) = self.outbox.inner.lock().unwrap().live.get_mut(r) {
            l.phase = phase;
        }
        self.outbox.changed.notify_waiters();
    }

    fn set_progress(&self, r: &str, bytes: u64) {
        if let Some(l) = self.outbox.inner.lock().unwrap().live.get_mut(r) {
            l.sent_bytes = bytes;
        }
    }

    fn progress_callback(self: &Arc<Self>, r: &str) -> crate::client::Progress {
        let engine = Arc::downgrade(self);
        let r = r.to_string();
        Arc::new(move |n| {
            let Some(e) = engine.upgrade() else { return };
            let token = {
                let mut inner = e.outbox.inner.lock().unwrap();
                let Some(l) = inner.live.get_mut(&r) else { return };
                l.sent_bytes += n;
                if l.last_emit.is_some_and(|t| t.elapsed() < PROGRESS_INTERVAL) {
                    return;
                }
                l.last_emit = Some(Instant::now());
                inner.items.get(&r).map(|i| i.token.clone())
            };
            if let Some(t) = token {
                e.outbox_changed(&t);
            }
        })
    }

    pub(crate) fn outbox_changed(&self, token: &str) {
        self.outbox.changed.notify_waiters();
        self.sink.emit(Event::OutboxChanged {
            token: token.to_string(),
            items: self.pending(token),
        });
    }

    fn pending_view(&self, item: &OutboxItem, live: &Live) -> PendingMessage {
        let state = match (item.failed, live.phase) {
            (Some(_), _) => PendingState::Failed,
            (None, Phase::Waiting) => PendingState::Waiting,
            // Queued behind a waiting message while offline: waiting as well.
            (None, Phase::Queued) if !self.is_online() => PendingState::Waiting,
            _ => PendingState::Sending,
        };
        let progress = match (&item.file, live.phase) {
            (Some(f), Phase::Uploading) if f.size > 0 => (live.sent_bytes as f32 / f.size as f32).min(1.0),
            (Some(f), _) if f.remote.is_some() => 1.0,
            _ => -1.0,
        };
        let me = self.user_id();
        PendingMessage {
            message: MessageView {
                id: -(item.seq as i64),
                token: item.token.clone(),
                actor_type: "users".into(),
                actor_id: me,
                author: String::new(),
                timestamp: item.created_at,
                text: item.text.clone(),
                kind: "text",
                system_message: String::new(),
                reference_id: item.reference_id.clone(),
                is_own: true,
                edited: false,
                markdown: true,
                reply_to: item.reply_to.clone(),
                reactions: Default::default(),
                reactions_self: Vec::new(),
                attachment: item.file.as_ref().map(|f| FileAttachment {
                    id: String::new(),
                    name: f.name.clone(),
                    path: f.remote.clone().unwrap_or_default(),
                    mimetype: f.mime.clone(),
                    size: f.size as i64,
                    width: f.width,
                    height: f.height,
                    preview_available: f.mime.starts_with("image/"),
                    link: String::new(),
                    voice_message: false,
                }),
            },
            state,
            progress,
            error: item.failed,
            local_path: item.file.as_ref().map(|f| f.local.clone()),
        }
    }
}

// --- retry policy --------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub(crate) enum Verdict {
    Retry { after: Option<Duration>, network: bool },
    Fail(SendError),
    /// Wait for a wake-up without counting an attempt (credentials rejected).
    Pause,
}

pub(crate) fn classify(e: &Error) -> Verdict {
    match e {
        Error::Network(_) | Error::Decode(_) => Verdict::Retry {
            after: None,
            network: true,
        },
        Error::RetryLater { after_secs, .. } => Verdict::Retry {
            after: after_secs.map(|s| Duration::from_secs(s.min(3600))),
            network: false,
        },
        Error::Http { status, .. } => match status {
            507 => Verdict::Fail(SendError::QuotaExceeded),
            408 | 425 | 500..=599 => Verdict::Retry {
                after: None,
                network: false,
            },
            403 => Verdict::Fail(SendError::Forbidden),
            413 => Verdict::Fail(SendError::TooLarge),
            _ => Verdict::Fail(SendError::Rejected),
        },
        Error::NotFound => Verdict::Fail(SendError::NotFound),
        Error::Unauthorized => Verdict::Pause,
        Error::LocalFile(_) => Verdict::Fail(SendError::FileMissing),
        Error::LoginExpired => Verdict::Fail(SendError::Rejected),
    }
}

/// 2^attempt seconds, at most [`MAX_BACKOFF_S`], scaled by 0.75..1.25 (`jitter` in 0..1).
pub(crate) fn backoff(attempt: u32, jitter: f64) -> Duration {
    let base = 1u64 << attempt.min(16);
    let secs = base.min(MAX_BACKOFF_S) as f64 * (0.75 + 0.5 * jitter.clamp(0.0, 1.0));
    Duration::from_millis((secs * 1000.0) as u64)
}

fn jitter() -> f64 {
    (uuid::Uuid::new_v4().as_u128() % 10_000) as f64 / 10_000.0
}

fn chunk_len(size: u64, index: u64) -> u64 {
    (size - index * CHUNK_SIZE).min(CHUNK_SIZE)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// File names must not contain path separators or start with a dot.
fn sanitize_name(name: &str) -> String {
    let n: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | '\0') { '_' } else { c })
        .collect();
    let n = n.trim().trim_start_matches('.').to_string();
    if n.is_empty() { "file".into() } else { n }
}

// --- storage -------------------------------------------------------------------------

struct Store {
    /// `None`: in memory only (no data dir given).
    dir: Option<PathBuf>,
}

impl Store {
    fn files_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("files"))
    }

    fn record(&self, r: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{r}.json")))
    }

    /// Loads all items; drops unreadable records and attachment files nobody refers to.
    fn load(&self) -> Vec<OutboxItem> {
        let Some(dir) = &self.dir else { return Vec::new() };
        let mut items = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let path = e.path();
                match path.extension().and_then(|x| x.to_str()) {
                    Some("json") => match std::fs::read(&path)
                        .ok()
                        .and_then(|b| serde_json::from_slice::<OutboxItem>(&b).ok())
                    {
                        Some(item) => items.push(item),
                        None => {
                            tracing::warn!("dropping unreadable outbox record {}", path.display());
                            let _ = std::fs::remove_file(&path);
                        }
                    },
                    Some("tmp") => {
                        let _ = std::fs::remove_file(&path);
                    }
                    _ => {}
                }
            }
        }
        if let Some(files) = self.files_dir() {
            if let Ok(entries) = std::fs::read_dir(&files) {
                for e in entries.flatten() {
                    let p = e.path().to_string_lossy().into_owned();
                    if !items.iter().any(|i| i.file.as_ref().is_some_and(|f| f.local == p)) {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
        items.sort_by_key(|i| i.seq);
        items
    }

    /// Atomic write: temp file, fsync, rename.
    fn save(&self, item: &OutboxItem) {
        let Some(path) = self.record(&item.reference_id) else { return };
        let tmp = path.with_extension("tmp");
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(path.parent().unwrap())?;
            let bytes = serde_json::to_vec(item).map_err(std::io::Error::other)?;
            {
                use std::io::Write;
                let mut f = std::fs::File::create(&tmp)?;
                f.write_all(&bytes)?;
                f.sync_all()?;
            }
            std::fs::rename(&tmp, &path)
        };
        if let Err(e) = write() {
            tracing::error!("cannot persist outbox item {}: {e}", item.reference_id);
        }
    }

    /// Forgets the record only (the item is delivered; its file may still be shown).
    fn remove_record(&self, r: &str) {
        if let Some(p) = self.record(r) {
            let _ = std::fs::remove_file(p);
        }
    }

    fn remove(&self, item: &OutboxItem) {
        self.remove_record(&item.reference_id);
        if let (Some(f), Some(files)) = (&item.file, self.files_dir()) {
            if Path::new(&f.local).starts_with(&files) {
                let _ = std::fs::remove_file(&f.local);
            }
        }
    }

    fn clear(&self) {
        if let Some(d) = &self.dir {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// Moves `src` into the outbox (copy + delete across file systems).
    fn adopt_file(&self, r: &str, src: &Path, name: &str) -> std::io::Result<PathBuf> {
        let Some(files) = self.files_dir() else {
            return Ok(src.to_path_buf());
        };
        std::fs::create_dir_all(&files)?;
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.len() <= 8 && e.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let dst = files.join(format!("{r}{ext}"));
        if std::fs::rename(src, &dst).is_err() {
            std::fs::copy(src, &dst)?;
            let _ = std::fs::remove_file(src);
        }
        Ok(dst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_bounded() {
        assert_eq!(backoff(0, 0.5), Duration::from_secs(1));
        assert_eq!(backoff(3, 0.5), Duration::from_secs(8));
        assert_eq!(backoff(10, 0.5), Duration::from_secs(60));
        assert_eq!(backoff(40, 1.0), Duration::from_secs(75));
        assert_eq!(backoff(40, 0.0), Duration::from_secs(45));
    }

    #[test]
    fn classifies_errors() {
        let http = |status| Error::Http {
            status,
            message: String::new(),
        };
        assert_eq!(classify(&http(502)), Verdict::Retry { after: None, network: false });
        assert_eq!(classify(&http(403)), Verdict::Fail(SendError::Forbidden));
        assert_eq!(classify(&http(507)), Verdict::Fail(SendError::QuotaExceeded));
        assert_eq!(classify(&http(413)), Verdict::Fail(SendError::TooLarge));
        assert_eq!(classify(&Error::NotFound), Verdict::Fail(SendError::NotFound));
        assert_eq!(classify(&Error::Unauthorized), Verdict::Pause);
        assert_eq!(
            classify(&Error::RetryLater { status: 429, after_secs: Some(7) }),
            Verdict::Retry { after: Some(Duration::from_secs(7)), network: false }
        );
        assert_eq!(classify(&Error::Decode("stalled".into())), Verdict::Retry { after: None, network: true });
        assert_eq!(classify(&Error::LocalFile("x".into())), Verdict::Fail(SendError::FileMissing));
    }

    #[test]
    fn chunk_lengths() {
        let size = 2 * CHUNK_SIZE + 5;
        assert_eq!(size.div_ceil(CHUNK_SIZE), 3);
        assert_eq!(chunk_len(size, 0), CHUNK_SIZE);
        assert_eq!(chunk_len(size, 2), 5);
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_name("../etc/passwd"), "_etc_passwd");
        assert_eq!(sanitize_name(".hidden"), "hidden");
        assert_eq!(sanitize_name("  "), "file");
        assert_eq!(sanitize_name("Foto 1.jpg"), "Foto 1.jpg");
    }

    fn item(r: &str, seq: u64, local: Option<String>) -> OutboxItem {
        OutboxItem {
            reference_id: r.into(),
            token: "tok".into(),
            seq,
            created_at: 1,
            expires_at: 2,
            text: "hi".into(),
            reply_to: None,
            file: local.map(|l| OutFile {
                local: l,
                name: "a.jpg".into(),
                mime: "image/jpeg".into(),
                size: 3,
                width: 0,
                height: 0,
                candidate: Some("/Talk/a.jpg".into()),
                name_n: 1,
                upload_id: None,
                remote: None,
            }),
            post_attempted: true,
            attempts: 2,
            failed: None,
        }
    }

    #[test]
    fn store_roundtrip_and_cleanup() {
        let dir = std::env::temp_dir().join(format!("outbox-test-{}", uuid::Uuid::new_v4()));
        let store = Store {
            dir: Some(dir.join("outbox")),
        };
        // A source file gets moved into the outbox.
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.jpg");
        std::fs::write(&src, b"abc").unwrap();
        let local = store.adopt_file("r1", &src, "Foto.jpg").unwrap();
        assert!(!src.exists());
        assert!(local.ends_with("files/r1.jpg"));

        store.save(&item("r2", 2, None));
        store.save(&item("r1", 1, Some(local.to_string_lossy().into_owned())));
        // Junk: a broken record, a stale temp file, an orphaned attachment.
        std::fs::write(dir.join("outbox/broken.json"), b"{").unwrap();
        std::fs::write(dir.join("outbox/x.tmp"), b"").unwrap();
        std::fs::write(dir.join("outbox/files/orphan.jpg"), b"").unwrap();

        let loaded = store.load();
        assert_eq!(loaded.iter().map(|i| i.reference_id.as_str()).collect::<Vec<_>>(), ["r1", "r2"]);
        assert!(loaded[0].post_attempted);
        assert_eq!(loaded[0].file.as_ref().unwrap().candidate.as_deref(), Some("/Talk/a.jpg"));
        assert!(!dir.join("outbox/broken.json").exists());
        assert!(!dir.join("outbox/x.tmp").exists());
        assert!(!dir.join("outbox/files/orphan.jpg").exists());
        assert!(local.exists());

        store.remove(&loaded[0]);
        assert!(!local.exists());
        assert_eq!(store.load().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
