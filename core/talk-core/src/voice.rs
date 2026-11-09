//! Voice messages: recording straight to MP3 on disk, drafts, and what playback needs
//! (downloads, duration and waveform).
//!
//! Design (patterns from Signal, adapted to Talk):
//! - The platform captures 16-bit mono PCM and pushes it every ~100 ms. The core encodes
//!   it to MP3 (LAME, CBR) and appends it to `{data_dir}/voice/{id}.mp3` right away. MP3
//!   consists of independent frames, so whatever reached the disk stays playable if the
//!   app is killed or crashes; nothing is held in memory.
//! - Talk only marks a share as voice message for `audio/mpeg` or `audio/wav` (spreed
//!   `SystemMessage/Listener`), hence real MP3 rather than AAC/Opus.
//! - Every recording is a draft until it is sent (one per conversation, like Signal). An
//!   interrupted recording (crash, kill, power loss after the last sync) becomes a draft
//!   on the next start: the file is cut after its last complete frame.
//! - Sending hands the file to the outbox, which uploads it like any attachment.
//! - Received voice messages are downloaded into `{data_dir}/files/` (resumable, LRU)
//!   and analysed once (duration, waveform), so they play offline afterwards.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use futures_util::StreamExt;
use mp3lame_encoder::{Bitrate, Builder, Encoder, FlushNoGap, Mode, MonoPcm, Quality};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::engine::{Engine, ReplyPreview};
use crate::error::{Error, Result};
use crate::outbox::PendingMessage;

/// Longest recording (Signal: one hour). `push` reports `limit_reached` from here on.
pub const MAX_DURATION_MS: u64 = 60 * 60 * 1000;
/// Shorter recordings are discarded (an accidental tap, like Signal).
pub const MIN_DURATION_MS: u64 = 1000;
pub const VOICE_MIME: &str = "audio/mpeg";
/// Bars of the waveform handed to the platform (it resamples to what it draws).
pub const WAVEFORM_BARS: usize = 100;

const BITRATE: Bitrate = Bitrate::Kbps64;
/// MPEG-1 at 32 kHz: plenty for speech, 288-byte frames of 36 ms at 64 kbit/s.
const OUT_RATE: u32 = 32_000;
/// One level value per window (live meter and waveform).
const WINDOW_MS: u64 = 100;
/// Level floor: quieter is drawn as silence (like Signal).
const FLOOR_DB: f32 = -60.0;
/// `fsync` interval while recording (survives power loss up to this much).
const SYNC_INTERVAL: Duration = Duration::from_secs(5);
/// Downloaded files kept for offline playback.
const FILES_LIMIT: u64 = 300 * 1024 * 1024;
/// A download whose body stops moving for this long fails (and resumes on retry).
const DOWNLOAD_STALL: Duration = Duration::from_secs(60);

/// A finished (or recovered) recording that has not been sent yet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceDraft {
    pub id: String,
    pub token: String,
    /// MP3 file, playable as is.
    pub path: String,
    pub duration_ms: u64,
    pub size: u64,
    /// [`WAVEFORM_BARS`] levels, 0 (silence) ..= 255 (full scale).
    pub waveform: Vec<u8>,
    /// Unix seconds.
    pub created_at: i64,
}

/// Returned by every [`VoiceRecorder::push`]: drives timer and level meter.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub duration_ms: u64,
    /// Current level 0..1 (dB scale, floor −60 dB).
    pub level: f32,
    /// [`MAX_DURATION_MS`] reached: further audio is dropped, call `finish`.
    pub limit_reached: bool,
}

/// Duration and waveform of an audio file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub duration_ms: u64,
    pub waveform: Vec<u8>,
}

/// Sidecar `{id}.json` of a recording.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    id: String,
    token: String,
    created_at: i64,
    sample_rate: u32,
    /// `false` while recording – after a restart: interrupted, to be recovered.
    finished: bool,
    #[serde(default)]
    duration_ms: u64,
    #[serde(default)]
    waveform: Vec<u8>,
}

// --- recording -----------------------------------------------------------------------

/// One recording in progress. Push PCM, then `finish` (draft) or `cancel`. Dropping an
/// unfinished recorder finishes it, so nothing recorded is lost.
pub struct VoiceRecorder {
    store: Arc<VoiceStore>,
    rec: Mutex<Rec>,
}

struct Rec {
    meta: Meta,
    /// `None` once finished or cancelled.
    encoder: Option<Encoder>,
    mp3: Option<File>,
    peaks: Option<File>,
    samples: u64,
    max_samples: u64,
    window: u64,
    /// Sum of squares of the current window (RMS level).
    win_sum: f64,
    win_fill: u64,
    levels: Vec<u8>,
    last_level: u8,
    last_sync: Instant,
    buf: Vec<u8>,
    /// First write error: recording stops, what is on disk stays a draft.
    failed: Option<String>,
}

impl VoiceRecorder {
    /// Encodes and stores `pcm` (16-bit mono at the rate given to `start_voice`).
    pub fn push(&self, pcm: &[i16]) -> Result<RecordingStatus> {
        let mut r = self.rec.lock().unwrap();
        if let Some(e) = &r.failed {
            return Err(Error::LocalFile(e.clone()));
        }
        if r.encoder.is_none() {
            return Err(Error::LocalFile("recording already finished".into()));
        }
        let room = r.max_samples.saturating_sub(r.samples) as usize;
        let pcm = &pcm[..pcm.len().min(room)];
        if !pcm.is_empty() {
            if let Err(e) = r.encode(pcm) {
                r.failed = Some(e.clone());
                return Err(Error::LocalFile(e));
            }
        }
        Ok(r.status())
    }

    /// Ends the recording. `None` when it was shorter than [`MIN_DURATION_MS`] (deleted).
    pub fn finish(&self) -> Result<Option<VoiceDraft>> {
        let mut r = self.rec.lock().unwrap();
        r.finish(&self.store)
    }

    /// Ends the recording and deletes it.
    pub fn cancel(&self) {
        let mut r = self.rec.lock().unwrap();
        r.encoder = None;
        r.mp3 = None;
        r.peaks = None;
        self.store.remove(&r.meta.id);
        self.store.active.lock().unwrap().remove(&r.meta.id);
    }

    pub fn status(&self) -> RecordingStatus {
        self.rec.lock().unwrap().status()
    }
}

impl Drop for VoiceRecorder {
    fn drop(&mut self) {
        let r = self.rec.get_mut().unwrap();
        if r.encoder.is_some() {
            let _ = r.finish(&self.store);
        }
    }
}

impl Rec {
    fn status(&self) -> RecordingStatus {
        RecordingStatus {
            duration_ms: self.samples * 1000 / self.meta.sample_rate as u64,
            level: self.last_level as f32 / 255.0,
            limit_reached: self.samples >= self.max_samples,
        }
    }

    fn encode(&mut self, pcm: &[i16]) -> std::result::Result<(), String> {
        let encoder = self.encoder.as_mut().ok_or("finished")?;
        self.buf.clear();
        self.buf.reserve(mp3lame_encoder::max_required_buffer_size(pcm.len()));
        encoder
            .encode_to_vec(MonoPcm(pcm), &mut self.buf)
            .map_err(|e| format!("mp3 encoder: {e:?}"))?;
        let mp3 = self.mp3.as_mut().ok_or("file closed")?;
        mp3.write_all(&self.buf).map_err(|e| format!("writing recording: {e}"))?;
        self.samples += pcm.len() as u64;

        let mut new_levels = Vec::new();
        for &s in pcm {
            let x = s as f64 / 32768.0;
            self.win_sum += x * x;
            self.win_fill += 1;
            if self.win_fill >= self.window {
                let level = level_byte((self.win_sum / self.win_fill as f64).sqrt() as f32);
                new_levels.push(level);
                self.last_level = level;
                self.win_sum = 0.0;
                self.win_fill = 0;
            }
        }
        if !new_levels.is_empty() {
            self.levels.extend_from_slice(&new_levels);
            if let Some(p) = self.peaks.as_mut() {
                let _ = p.write_all(&new_levels);
            }
        }
        if self.last_sync.elapsed() >= SYNC_INTERVAL {
            self.last_sync = Instant::now();
            let _ = self.mp3.as_ref().map(File::sync_data);
            let _ = self.peaks.as_ref().map(File::sync_data);
        }
        Ok(())
    }

    fn finish(&mut self, store: &VoiceStore) -> Result<Option<VoiceDraft>> {
        let Some(mut encoder) = self.encoder.take() else {
            return Err(Error::LocalFile("recording already finished".into()));
        };
        store.active.lock().unwrap().remove(&self.meta.id);
        if self.failed.is_none() {
            self.buf.clear();
            self.buf.reserve(7200);
            if encoder.flush_to_vec::<FlushNoGap>(&mut self.buf).is_ok() {
                if let Some(f) = self.mp3.as_mut() {
                    let _ = f.write_all(&self.buf);
                }
            }
        }
        if let Some(f) = self.mp3.take() {
            let _ = f.sync_all();
        }
        self.peaks = None;
        let path = store.mp3_path(&self.meta.id);
        // After a write error the file holds less than was pushed: count its frames.
        let duration_ms = if self.failed.is_some() {
            repair_mp3(&path).map(|s| s.duration_ms()).unwrap_or(0)
        } else {
            self.samples * 1000 / self.meta.sample_rate as u64
        };
        if duration_ms < MIN_DURATION_MS {
            store.remove(&self.meta.id);
            return Ok(None);
        }
        self.meta.finished = true;
        self.meta.duration_ms = duration_ms;
        self.meta.waveform = bars(&self.levels, WAVEFORM_BARS);
        store.save(&self.meta).map_err(|e| Error::LocalFile(format!("saving draft: {e}")))?;
        store.drop_other_drafts(&self.meta.token, &self.meta.id);
        Ok(store.draft_view(&self.meta))
    }
}

/// RMS amplitude 0..1 → 0..=255 on a dB scale with [`FLOOR_DB`] as zero.
fn level_byte(rms: f32) -> u8 {
    if rms <= 0.0 {
        return 0;
    }
    let db = 20.0 * rms.log10();
    (((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `n` bars from per-window levels (mean per bar), scaled so the loudest bar is full
/// height (like Signal: the shape matters, not the recording level).
fn bars(levels: &[u8], n: usize) -> Vec<u8> {
    if levels.is_empty() {
        return vec![0; n];
    }
    let raw: Vec<u8> = (0..n)
        .map(|i| {
            let a = i * levels.len() / n;
            let b = ((i + 1) * levels.len() / n).max(a + 1).min(levels.len());
            let part = &levels[a.min(levels.len() - 1)..b];
            (part.iter().map(|&v| v as u32).sum::<u32>() / part.len() as u32) as u8
        })
        .collect();
    let max = raw.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return raw;
    }
    raw.into_iter().map(|v| (v as u32 * 255 / max as u32) as u8).collect()
}

// --- MP3 frames ----------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Mp3Scan {
    /// Bytes up to the end of the last complete frame.
    valid_len: u64,
    samples: u64,
    sample_rate: u32,
}

impl Mp3Scan {
    fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 { 0 } else { self.samples * 1000 / self.sample_rate as u64 }
    }
}

/// (frame length in bytes, samples per frame, sample rate) of an MPEG audio Layer III
/// frame header.
fn frame_header(h: [u8; 4]) -> Option<(u64, u64, u32)> {
    if h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (h[1] >> 3) & 0b11; // 0: 2.5, 2: 2, 3: 1
    let layer = (h[1] >> 1) & 0b11; // 1: Layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let br_idx = (h[2] >> 4) as usize;
    let sr_idx = ((h[2] >> 2) & 0b11) as usize;
    let padding = ((h[2] >> 1) & 1) as u64;
    if br_idx == 0 || br_idx == 15 || sr_idx == 3 {
        return None;
    }
    const BR_V1: [u64; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    const BR_V2: [u64; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    const SR_V1: [u32; 3] = [44100, 48000, 32000];
    let (bitrate, rate, samples, coeff) = match version {
        3 => (BR_V1[br_idx] * 1000, SR_V1[sr_idx], 1152, 144),
        2 => (BR_V2[br_idx] * 1000, SR_V1[sr_idx] / 2, 576, 72),
        _ => (BR_V2[br_idx] * 1000, SR_V1[sr_idx] / 4, 576, 72),
    };
    Some((coeff * bitrate / rate as u64 + padding, samples, rate))
}

/// Counts the complete frames of an MP3 file without decoding.
fn scan_mp3(file: &mut File) -> std::io::Result<Mp3Scan> {
    let len = file.metadata()?.len();
    let mut scan = Mp3Scan::default();
    let mut pos = 0u64;
    let mut h = [0u8; 4];
    while pos + 4 <= len {
        file.seek(SeekFrom::Start(pos))?;
        file.read_exact(&mut h)?;
        let Some((frame, samples, rate)) = frame_header(h) else { break };
        if pos + frame > len {
            break; // cut off mid-frame
        }
        pos += frame;
        scan.valid_len = pos;
        scan.samples += samples;
        scan.sample_rate = rate;
    }
    Ok(scan)
}

/// Cuts a (possibly interrupted) recording after its last complete frame.
fn repair_mp3(path: &Path) -> std::io::Result<Mp3Scan> {
    let mut f = File::options().read(true).write(true).open(path)?;
    let scan = scan_mp3(&mut f)?;
    if scan.valid_len < f.metadata()?.len() {
        f.set_len(scan.valid_len)?;
        f.sync_all()?;
    }
    Ok(scan)
}

// --- drafts on disk ------------------------------------------------------------------

pub(crate) struct VoiceStore {
    dir: PathBuf,
    files: PathBuf,
    /// Recordings in progress in this process (not to be recovered).
    active: Mutex<HashSet<String>>,
    downloads: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
    /// Analysed files, by `path|size` (backed by the media cache).
    infos: Mutex<HashMap<String, AudioInfo>>,
}

impl VoiceStore {
    /// Without a data dir everything goes to the temp dir (CLI, tests).
    pub(crate) fn open(data_dir: Option<&Path>) -> Self {
        let base = data_dir.map(Path::to_path_buf).unwrap_or_else(|| std::env::temp_dir().join("talk-core"));
        let store = Self {
            dir: base.join("voice"),
            files: base.join("files"),
            active: Mutex::default(),
            downloads: Mutex::default(),
            infos: Mutex::default(),
        };
        store.cleanup();
        store
    }

    fn mp3_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.mp3"))
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn peaks_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.peaks"))
    }

    /// Drops temp files and recordings without a sidecar (never started properly).
    fn cleanup(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            match p.extension().and_then(|x| x.to_str()) {
                Some("tmp") => {
                    let _ = std::fs::remove_file(&p);
                }
                Some("mp3" | "peaks") if !self.meta_path(&stem).exists() => {
                    let _ = std::fs::remove_file(&p);
                }
                _ => {}
            }
        }
    }

    fn metas(&self) -> Vec<Meta> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .filter_map(|e| {
                let m = std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<Meta>(&b).ok());
                if m.is_none() {
                    let _ = std::fs::remove_file(e.path());
                }
                m
            })
            .collect()
    }

    /// Atomic write of a sidecar.
    fn save(&self, meta: &Meta) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.meta_path(&meta.id);
        let tmp = path.with_extension("tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&serde_json::to_vec(meta).map_err(std::io::Error::other)?)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &path)
    }

    fn remove(&self, id: &str) {
        for p in [self.mp3_path(id), self.peaks_path(id), self.meta_path(id)] {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Forgets the sidecar files only (the MP3 was handed to the outbox).
    fn forget(&self, id: &str) {
        for p in [self.peaks_path(id), self.meta_path(id)] {
            let _ = std::fs::remove_file(p);
        }
    }

    fn drop_other_drafts(&self, token: &str, keep: &str) {
        let active = self.active.lock().unwrap().clone();
        for m in self.metas() {
            if m.token == token && m.id != keep && !active.contains(&m.id) {
                self.remove(&m.id);
            }
        }
    }

    fn draft_view(&self, m: &Meta) -> Option<VoiceDraft> {
        let path = self.mp3_path(&m.id);
        let size = std::fs::metadata(&path).ok()?.len();
        Some(VoiceDraft {
            id: m.id.clone(),
            token: m.token.clone(),
            path: path.to_string_lossy().into_owned(),
            duration_ms: m.duration_ms,
            size,
            waveform: m.waveform.clone(),
            created_at: m.created_at,
        })
    }

    /// Turns an interrupted recording into a draft (`None`: nothing usable, deleted).
    fn recover(&self, mut m: Meta) -> Option<Meta> {
        let scan = repair_mp3(&self.mp3_path(&m.id)).unwrap_or_default();
        let duration_ms = scan.duration_ms();
        if duration_ms < MIN_DURATION_MS {
            self.remove(&m.id);
            return None;
        }
        tracing::info!(id = %m.id, duration_ms, "recovered interrupted voice recording");
        let levels = std::fs::read(self.peaks_path(&m.id)).unwrap_or_default();
        m.finished = true;
        m.duration_ms = duration_ms;
        m.waveform = bars(&levels, WAVEFORM_BARS);
        self.save(&m).ok()?;
        Some(m)
    }

    /// The draft of a conversation (newest one; older ones are dropped).
    fn draft(&self, token: &str) -> Option<VoiceDraft> {
        let active = self.active.lock().unwrap().clone();
        let mut metas: Vec<Meta> = self
            .metas()
            .into_iter()
            .filter(|m| m.token == token && !active.contains(&m.id))
            .filter_map(|m| if m.finished { Some(m) } else { self.recover(m) })
            .collect();
        metas.sort_by_key(|m| m.created_at);
        let newest = metas.pop()?;
        for old in metas {
            self.remove(&old.id);
        }
        match self.draft_view(&newest) {
            Some(d) => Some(d),
            None => {
                self.remove(&newest.id); // audio file gone
                None
            }
        }
    }

    /// Conversations that have a draft (for the room list).
    fn draft_tokens(&self) -> Vec<String> {
        let active = self.active.lock().unwrap().clone();
        let mut t: Vec<String> = self
            .metas()
            .into_iter()
            .filter(|m| !active.contains(&m.id))
            .map(|m| m.token)
            .collect();
        t.sort();
        t.dedup();
        t
    }

    /// Local file for a remote file (by user, path and size: a replaced file is new).
    fn file_path(&self, user: &str, path: &str, size: i64) -> PathBuf {
        let path = path.trim_start_matches('/');
        let digest = Sha256::digest(format!("{user}\n{path}\n{size}").as_bytes());
        let name: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(|e| format!(".{}", e.to_ascii_lowercase()))
            .unwrap_or_default();
        self.files.join(format!("{name}{ext}"))
    }

    pub(crate) fn prune_files(&self) {
        // Partial downloads count too; the oldest go first.
        crate::cache::prune(&self.files, FILES_LIMIT);
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn info_key(path: &Path) -> Option<String> {
    let len = std::fs::metadata(path).ok()?.len();
    // v3: RMS levels, mean per bar, normalised.
    Some(format!("audio/v3/{}|{len}", path.display()))
}

// --- engine side ---------------------------------------------------------------------

impl Engine {
    /// Starts recording a voice message for `token` at `sample_rate` Hz (16-bit mono).
    /// An existing draft of that conversation is replaced once this one is finished.
    pub fn start_voice(&self, token: &str, sample_rate: u32) -> Result<VoiceRecorder> {
        if !(8_000..=48_000).contains(&sample_rate) {
            return Err(Error::LocalFile(format!("unsupported sample rate {sample_rate}")));
        }
        let store = self.voice.clone();
        std::fs::create_dir_all(&store.dir).map_err(|e| Error::LocalFile(format!("{}: {e}", store.dir.display())))?;
        let encoder = (|| {
            let mut b = Builder::new().ok_or("cannot create encoder")?;
            b.set_num_channels(1).map_err(|e| format!("{e:?}"))?;
            b.set_mode(Mode::Mono).map_err(|e| format!("{e:?}"))?;
            b.set_sample_rate(sample_rate).map_err(|e| format!("{e:?}"))?;
            b.set_output_sample_rate(NonZeroU32::new(OUT_RATE)).map_err(|e| format!("{e:?}"))?;
            b.set_brate(BITRATE).map_err(|e| format!("{e:?}"))?;
            b.set_quality(Quality::Good).map_err(|e| format!("{e:?}"))?;
            // Plain CBR frames: no Xing/LAME header to patch at the end.
            b.set_to_write_vbr_tag(false).map_err(|e| format!("{e:?}"))?;
            b.build().map_err(|e| format!("{e:?}"))
        })()
        .map_err(|e: String| Error::LocalFile(format!("mp3 encoder: {e}")))?;

        let id = uuid::Uuid::new_v4().simple().to_string();
        let meta = Meta {
            id: id.clone(),
            token: token.to_string(),
            created_at: unix_now(),
            sample_rate,
            finished: false,
            duration_ms: 0,
            waveform: Vec::new(),
        };
        let io = |e: std::io::Error| Error::LocalFile(format!("creating recording: {e}"));
        // The sidecar first: an MP3 without one is junk, one with it gets recovered.
        store.save(&meta).map_err(io)?;
        let mp3 = File::create(store.mp3_path(&id)).map_err(io)?;
        let peaks = File::create(store.peaks_path(&id)).ok();
        store.active.lock().unwrap().insert(id);
        Ok(VoiceRecorder {
            store: store.clone(),
            rec: Mutex::new(Rec {
                meta,
                encoder: Some(encoder),
                mp3: Some(mp3),
                peaks,
                samples: 0,
                max_samples: MAX_DURATION_MS * sample_rate as u64 / 1000,
                window: (sample_rate as u64 * WINDOW_MS / 1000).max(1),
                win_sum: 0.0,
                win_fill: 0,
                levels: Vec::new(),
                last_level: 0,
                last_sync: Instant::now(),
                buf: Vec::new(),
                failed: None,
            }),
        })
    }

    /// The unsent recording of a conversation, recovering an interrupted one. May read
    /// the whole file once after a crash: call off the UI thread.
    pub fn voice_draft(&self, token: &str) -> Option<VoiceDraft> {
        self.voice.draft(token)
    }

    /// Conversations with an unsent recording.
    pub fn voice_draft_tokens(&self) -> Vec<String> {
        self.voice.draft_tokens()
    }

    pub fn discard_voice_draft(&self, token: &str) {
        if let Some(d) = self.voice.draft(token) {
            self.voice.remove(&d.id);
        }
    }

    /// Sends the draft of `token` through the outbox as a voice message. `name` is the
    /// file name (the platform formats the local date; `.mp3` is enforced).
    pub fn send_voice_draft(
        self: &Arc<Self>,
        token: &str,
        name: &str,
        reply_to: Option<ReplyPreview>,
        reference_id: &str,
    ) -> Result<PendingMessage> {
        let draft = self.voice.draft(token).ok_or(Error::NotFound)?;
        let stem = name.trim().trim_end_matches(".mp3").trim();
        let name = format!("{}.mp3", if stem.is_empty() { "Voice message" } else { stem });
        let info = AudioInfo {
            duration_ms: draft.duration_ms,
            waveform: draft.waveform.clone(),
        };
        let pending =
            self.enqueue_file_inner(token, &draft.path, &name, VOICE_MIME, 0, 0, "", reply_to, reference_id, true)?;
        self.voice.forget(&draft.id);
        if let Some(local) = &pending.local_path {
            self.remember_info(Path::new(local), info);
        }
        Ok(pending)
    }

    /// Sends an existing audio file as voice message (moved into the outbox). The
    /// server keeps the voice flag only for `.mp3` (audio/mpeg) and `.wav` files.
    pub fn enqueue_voice_file(
        self: &Arc<Self>,
        token: &str,
        local_path: &str,
        name: &str,
        reply_to: Option<ReplyPreview>,
        reference_id: &str,
    ) -> Result<PendingMessage> {
        let mime = if name.to_ascii_lowercase().ends_with(".wav") { "audio/wav" } else { VOICE_MIME };
        self.enqueue_file_inner(token, local_path, name, mime, 0, 0, "", reply_to, reference_id, true)
    }

    /// Delivered voice message: keep the local copy as the downloaded file, so playing
    /// one's own message needs no download.
    pub(crate) fn keep_sent_voice(&self, local: &str, remote: &str, size: u64) {
        let dst = self.voice.file_path(&self.user_id(), remote, size as i64);
        let src = Path::new(local);
        if dst.exists() || !src.is_file() {
            return;
        }
        let _ = std::fs::create_dir_all(&self.voice.files);
        if std::fs::hard_link(src, &dst).is_err() && std::fs::copy(src, &dst).is_err() {
            return;
        }
        let info = info_key(src).and_then(|k| self.voice.infos.lock().unwrap().get(&k).cloned());
        if let Some(info) = info {
            self.remember_info(&dst, info);
        }
    }

    fn remember_info(&self, path: &Path, info: AudioInfo) {
        let Some(key) = info_key(path) else { return };
        self.voice.infos.lock().unwrap().insert(key.clone(), info.clone());
        let engine = self.weak_self();
        self.spawn_task(async move {
            if let (Some(e), Ok(json)) = (engine.upgrade(), serde_json::to_vec(&info)) {
                e.cache.media_put(&key, &json, "application/json").await;
            }
        });
    }

    /// Duration and waveform of a local audio file (decoded once, then cached).
    pub async fn audio_info(&self, local_path: &str) -> Result<AudioInfo> {
        let path = PathBuf::from(local_path);
        let key = info_key(&path).ok_or_else(|| Error::LocalFile(format!("{local_path} missing")))?;
        if let Some(i) = self.voice.infos.lock().unwrap().get(&key) {
            return Ok(i.clone());
        }
        if let Some((data, _)) = self.cache.media_get(&key).await {
            if let Ok(i) = serde_json::from_slice::<AudioInfo>(&data) {
                self.voice.infos.lock().unwrap().insert(key, i.clone());
                return Ok(i);
            }
        }
        let p = path.clone();
        let info = tokio::task::spawn_blocking(move || analyze(&p))
            .await
            .map_err(|e| Error::Decode(format!("audio analysis: {e}")))??;
        self.remember_info(&path, info.clone());
        Ok(info)
    }

    /// Local copy of a shared file (`path` below the user root as in the message, `size`
    /// from the message). Downloads it once – resumable after an interruption – and
    /// keeps it for offline use (LRU). Concurrent calls for one file share the download.
    pub async fn fetch_file(&self, path: &str, size: i64) -> Result<String> {
        self.ensure_user().await.ok();
        let dst = self.voice.file_path(&self.user_id(), path, size);
        let complete = |p: &Path| std::fs::metadata(p).is_ok_and(|m| size <= 0 || m.len() == size as u64);
        if complete(&dst) {
            touch(&dst);
            return Ok(dst.to_string_lossy().into_owned());
        }
        let lock = self
            .voice
            .downloads
            .lock()
            .unwrap()
            .entry(dst.clone())
            .or_default()
            .clone();
        let _guard = lock.lock().await;
        if complete(&dst) {
            return Ok(dst.to_string_lossy().into_owned());
        }
        self.ensure_user().await?;
        let remote = format!("/{}", path.trim_start_matches('/'));
        let part = PathBuf::from(format!("{}.part", dst.display()));
        tokio::fs::create_dir_all(&self.voice.files)
            .await
            .map_err(|e| Error::LocalFile(format!("{}: {e}", self.voice.files.display())))?;
        let r = self.download(&remote, &part, size).await;
        let r = self.guard(r);
        r?;
        tokio::fs::rename(&part, &dst)
            .await
            .map_err(|e| Error::LocalFile(format!("{}: {e}", dst.display())))?;
        Ok(dst.to_string_lossy().into_owned())
    }

    async fn download(&self, remote: &str, part: &Path, size: i64) -> Result<()> {
        let user = self.user_id();
        let io = |e: std::io::Error| Error::LocalFile(format!("{}: {e}", part.display()));
        for _ in 0..2 {
            let offset = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
            if size > 0 && offset >= size as u64 {
                // Complete (or too long: start over).
                if offset == size as u64 {
                    return Ok(());
                }
                let _ = tokio::fs::remove_file(part).await;
                continue;
            }
            let resp = self.client.download(&user, remote, offset).await?;
            let status = resp.status().as_u16();
            if status == 416 {
                let _ = tokio::fs::remove_file(part).await;
                continue;
            }
            let resume = status == 206 && offset > 0;
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(resume)
                .truncate(!resume)
                .open(part)
                .await
                .map_err(io)?;
            if resume {
                tracing::info!("resuming download of {remote} at {offset}");
            }
            let mut body = resp.bytes_stream();
            loop {
                match tokio::time::timeout(DOWNLOAD_STALL, body.next()).await {
                    Err(_) => return Err(Error::Decode(format!("download of {remote} stalled"))),
                    Ok(None) => break,
                    Ok(Some(chunk)) => file.write_all(&chunk?).await.map_err(io)?,
                }
            }
            file.sync_all().await.map_err(io)?;
            let got = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
            if size > 0 && got < size as u64 {
                return Err(Error::Decode(format!("download of {remote} incomplete ({got}/{size})")));
            }
            return Ok(());
        }
        Err(Error::Decode(format!("download of {remote} failed")))
    }
}

fn touch(p: &Path) {
    let _ = File::options().write(true).open(p).and_then(|f| f.set_modified(SystemTime::now()));
}

// --- decoding ------------------------------------------------------------------------

/// Decodes an audio file (MP3, WAV, AAC/MP4 – the official Talk app sends AAC named
/// `.mp3`) for its exact duration and waveform.
pub fn analyze(path: &Path) -> Result<AudioInfo> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
    use symphonia::core::errors::Error as SymError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let bad = |e: SymError| Error::Decode(format!("{}: {e}", path.display()));
    let file = File::open(path).map_err(|e| Error::LocalFile(format!("{}: {e}", path.display())))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(bad)?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| Error::Decode(format!("{}: no audio track", path.display())))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(bad)?;

    let mut frames = 0u64;
    let mut rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut levels = Vec::new();
    let (mut win_sum, mut win_fill) = (0f64, 0u64);
    let mut samples: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(_)) | Err(SymError::ResetRequired) => break,
            Err(e) if frames > 0 => {
                tracing::warn!("{}: stopping at {e}", path.display());
                break;
            }
            Err(e) => return Err(bad(e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(bad(e)),
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        let channels = spec.channels.count().max(1);
        let n = decoded.frames();
        if samples.as_ref().is_none_or(|s| s.capacity() < n * channels) {
            samples = Some(SampleBuffer::new(decoded.capacity() as u64, spec));
        }
        let buf = samples.as_mut().unwrap();
        buf.copy_interleaved_ref(decoded);
        let window = (rate as u64 * WINDOW_MS / 1000).max(1);
        for frame in buf.samples().chunks(channels) {
            win_sum += (frame[0] as f64) * (frame[0] as f64);
            win_fill += 1;
            if win_fill >= window {
                levels.push(level_byte((win_sum / win_fill as f64).sqrt() as f32));
                win_sum = 0.0;
                win_fill = 0;
            }
        }
        frames += n as u64;
    }
    if rate == 0 || frames == 0 {
        return Err(Error::Decode(format!("{}: no audio", path.display())));
    }
    if win_fill > 0 {
        levels.push(level_byte((win_sum / win_fill as f64).sqrt() as f32));
    }
    Ok(AudioInfo {
        duration_ms: frames * 1000 / rate as u64,
        waveform: bars(&levels, WAVEFORM_BARS),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("voice-test-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Speech-like test signal: a 220 Hz tone, amplitude modulated at 3 Hz.
    fn tone(rate: u32, start: u64, n: usize) -> Vec<i16> {
        (0..n as u64)
            .map(|i| {
                let t = (start + i) as f32 / rate as f32;
                let env = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 3.0 * t).sin();
                (env * 12000.0 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()) as i16
            })
            .collect()
    }

    fn recorder(store: &Arc<VoiceStore>, token: &str, rate: u32) -> VoiceRecorder {
        // Same as Engine::start_voice without an engine.
        let mut b = Builder::new().unwrap();
        b.set_num_channels(1).unwrap();
        b.set_mode(Mode::Mono).unwrap();
        b.set_sample_rate(rate).unwrap();
        b.set_output_sample_rate(NonZeroU32::new(OUT_RATE)).unwrap();
        b.set_brate(BITRATE).unwrap();
        b.set_quality(Quality::Good).unwrap();
        b.set_to_write_vbr_tag(false).unwrap();
        let id = uuid::Uuid::new_v4().simple().to_string();
        let meta = Meta {
            id: id.clone(),
            token: token.into(),
            created_at: unix_now(),
            sample_rate: rate,
            finished: false,
            duration_ms: 0,
            waveform: Vec::new(),
        };
        store.save(&meta).unwrap();
        store.active.lock().unwrap().insert(id.clone());
        VoiceRecorder {
            store: store.clone(),
            rec: Mutex::new(Rec {
                meta,
                encoder: Some(b.build().unwrap()),
                mp3: Some(File::create(store.mp3_path(&id)).unwrap()),
                peaks: File::create(store.peaks_path(&id)).ok(),
                samples: 0,
                max_samples: MAX_DURATION_MS * rate as u64 / 1000,
                window: rate as u64 * WINDOW_MS / 1000,
                win_sum: 0.0,
                win_fill: 0,
                levels: Vec::new(),
                last_level: 0,
                last_sync: Instant::now(),
                buf: Vec::new(),
                failed: None,
            }),
        }
    }

    /// Pushes `secs` seconds in 100 ms blocks.
    fn record(r: &VoiceRecorder, rate: u32, secs: u64) -> RecordingStatus {
        let block = (rate / 10) as usize;
        let mut status = r.status();
        for i in 0..secs * 10 {
            status = r.push(&tone(rate, i * block as u64, block)).unwrap();
        }
        status
    }

    #[test]
    fn frame_headers() {
        // MPEG-1 Layer III, 64 kbit/s, 32 kHz, no padding: 288 bytes, 1152 samples.
        assert_eq!(frame_header([0xFF, 0xFB, 0x58, 0xC4]), Some((288, 1152, 32000)));
        // 128 kbit/s, 44.1 kHz, padding.
        assert_eq!(frame_header([0xFF, 0xFB, 0x92, 0x64]), Some((418, 1152, 44100)));
        assert_eq!(frame_header([0x49, 0x44, 0x33, 0x04]), None); // "ID3"
        assert_eq!(frame_header([0xFF, 0xFB, 0xF0, 0x00]), None); // bad bitrate
    }

    #[test]
    fn levels_and_bars() {
        assert_eq!(level_byte(0.0), 0);
        assert_eq!(level_byte(1.0), 255);
        assert_eq!(level_byte(0.001), 0); // −60 dB
        assert!(level_byte(0.1) > 100 && level_byte(0.1) < 200);
        assert_eq!(bars(&[], 4), vec![0; 4]);
        assert_eq!(bars(&[1, 9, 3, 4], 2), vec![255, 153]);
        assert_eq!(bars(&[0, 0], 2), vec![0, 0]);
        assert_eq!(bars(&[5, 10], 4), vec![127, 127, 255, 255]);
    }

    #[test]
    fn records_twenty_minutes_exactly() {
        let d = dir("long");
        let store = Arc::new(VoiceStore::open(Some(&d)));
        let rate = 44_100;
        let r = recorder(&store, "tok", rate);
        let status = record(&r, rate, 20 * 60);
        assert_eq!(status.duration_ms, 20 * 60 * 1000);
        assert!(status.level > 0.5, "level {}", status.level);
        let draft = r.finish().unwrap().unwrap();
        assert_eq!(draft.duration_ms, 20 * 60 * 1000);
        assert_eq!(draft.waveform.len(), WAVEFORM_BARS);
        // CBR 64 kbit/s: 8000 bytes per second.
        let expected = 20 * 60 * 8000;
        assert!(draft.size.abs_diff(expected) < 8000, "size {}", draft.size);
        // Every frame is intact; decoding gives the full length.
        let scan = scan_mp3(&mut File::open(&draft.path).unwrap()).unwrap();
        assert_eq!(scan.valid_len, draft.size);
        let info = analyze(Path::new(&draft.path)).unwrap();
        assert!(info.duration_ms.abs_diff(draft.duration_ms) < 100, "decoded {}", info.duration_ms);
        assert!(info.waveform.iter().all(|&b| b > 100), "{:?}", info.waveform);
        assert_eq!(store.draft("tok"), Some(draft));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stops_at_one_hour() {
        let d = dir("limit");
        let store = Arc::new(VoiceStore::open(Some(&d)));
        let rate = 8_000;
        let r = recorder(&store, "tok", rate);
        let status = record(&r, rate, 60 * 60 + 5);
        assert!(status.limit_reached);
        assert_eq!(status.duration_ms, MAX_DURATION_MS);
        let draft = r.finish().unwrap().unwrap();
        assert_eq!(draft.duration_ms, MAX_DURATION_MS);
        let info = analyze(Path::new(&draft.path)).unwrap();
        assert!(info.duration_ms.abs_diff(MAX_DURATION_MS) < 100, "decoded {}", info.duration_ms);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn short_recordings_are_dropped_and_cancel_deletes() {
        let d = dir("short");
        let store = Arc::new(VoiceStore::open(Some(&d)));
        let r = recorder(&store, "tok", 48_000);
        r.push(&tone(48_000, 0, 24_000)).unwrap(); // 0.5 s
        assert_eq!(r.finish().unwrap(), None);
        let r = recorder(&store, "tok", 48_000);
        record(&r, 48_000, 3);
        r.cancel();
        drop(r);
        assert_eq!(store.draft("tok"), None);
        assert_eq!(std::fs::read_dir(&store.dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn interrupted_recording_becomes_a_draft() {
        let d = dir("crash");
        let rate = 48_000;
        let (id, path) = {
            let store = Arc::new(VoiceStore::open(Some(&d)));
            let r = recorder(&store, "tok", rate);
            record(&r, rate, 90);
            let mut rec = r.rec.lock().unwrap();
            // Simulated kill: files closed without finishing, encoder state lost.
            rec.encoder = None;
            rec.mp3 = None;
            rec.peaks = None;
            (rec.meta.id.clone(), store.mp3_path(&rec.meta.id))
        };
        // Cut mid-frame, like a write interrupted by the kill.
        let len = std::fs::metadata(&path).unwrap().len();
        File::options().write(true).open(&path).unwrap().set_len(len - 100).unwrap();

        let store = VoiceStore::open(Some(&d));
        let draft = store.draft("tok").expect("recovered");
        assert_eq!(draft.id, id);
        // Up to the last complete frame (36 ms) plus the encoder's look-ahead is lost.
        assert!(draft.duration_ms > 89_500 && draft.duration_ms <= 90_000, "{}", draft.duration_ms);
        assert_eq!(draft.waveform.len(), WAVEFORM_BARS);
        assert!(draft.waveform.iter().all(|&b| b > 100));
        let info = analyze(Path::new(&draft.path)).unwrap();
        assert!(info.duration_ms.abs_diff(draft.duration_ms) < 100);
        // Second open: already a finished draft, unchanged.
        assert_eq!(VoiceStore::open(Some(&d)).draft("tok"), Some(draft));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dropped_recorder_keeps_its_draft_and_newest_wins() {
        let d = dir("drop");
        let store = Arc::new(VoiceStore::open(Some(&d)));
        let r = recorder(&store, "tok", 16_000);
        record(&r, 16_000, 2);
        drop(r); // platform lost the handle
        let first = store.draft("tok").unwrap();
        let r = recorder(&store, "tok", 16_000);
        record(&r, 16_000, 4);
        let second = r.finish().unwrap().unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(store.draft("tok"), Some(second));
        assert!(!Path::new(&first.path).exists());
        assert_eq!(store.draft_tokens(), vec!["tok".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cache_file_names() {
        let store = VoiceStore::open(Some(&dir("names")));
        let a = store.file_path("u", "/Talk/A.MP3", 10);
        assert_eq!(a, store.file_path("u", "Talk/A.MP3", 10));
        assert_ne!(a, store.file_path("u", "Talk/A.MP3", 11));
        assert!(a.to_string_lossy().ends_with(".mp3"));
    }
}
