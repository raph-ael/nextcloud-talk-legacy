//! Call recording: starting/stopping (moderators) and the notices afterwards.
//!
//! The recording itself runs on the server (recording backend); the call only shows its
//! status ([`super::CallView::recording`]). When the file is uploaded, the user who
//! started the recording gets a notification with "share to chat" / "dismiss" actions
//! (also for the transcript and the summary). The core turns those into
//! [`RecordingNotice`]s; the actions are the links the server put into the notification.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use super::CallRecording;
use crate::engine::{Engine, Event};
use crate::error::{Error, Result};

/// After a recording of ours ended, look for its notice this often (fallback when
/// notify_push is missing) …
const NOTICE_POLL: Duration = Duration::from_secs(30);
/// … for this long (long recordings take a while to upload).
const NOTICE_POLL_FOR: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordingNoticeKind {
    /// "Aufnahme verfügbar"
    Recording,
    /// "Transkript verfügbar"
    Transcript,
    /// "Zusammenfassung verfügbar"
    Summary,
    /// The recording server could not upload the recording.
    RecordingFailed,
    TranscriptFailed,
    SummaryFailed,
}

impl RecordingNotice {
    /// Short German label (CLI, logs).
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            RecordingNoticeKind::Recording => "Aufnahme",
            RecordingNoticeKind::Transcript => "Transkript",
            RecordingNoticeKind::Summary => "Zusammenfassung",
            RecordingNoticeKind::RecordingFailed => "Upload-Fehler",
            RecordingNoticeKind::TranscriptFailed => "Transkript-Fehler",
            RecordingNoticeKind::SummaryFailed => "Zusammenf.-Fehler",
        }
    }
}

impl RecordingNoticeKind {
    pub fn is_failure(self) -> bool {
        matches!(self, Self::RecordingFailed | Self::TranscriptFailed | Self::SummaryFailed)
    }
}

/// A finished recording (or transcript/summary, or a failure) waiting for the user.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingNotice {
    /// Notification id: pass it to `share_recording` / `dismiss_recording`.
    pub id: i64,
    pub token: String,
    pub room_name: String,
    pub kind: RecordingNoticeKind,
    /// File name in the user's files (empty for an upload failure).
    pub file_name: String,
    /// Unix seconds when it was stored.
    pub timestamp: i64,
    /// "In Chat teilen" is possible (otherwise only dismiss).
    pub can_share: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Action {
    method: String,
    link: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Stored {
    notice: RecordingNotice,
    share: Option<Action>,
    dismiss: Option<Action>,
}

#[derive(Default)]
pub(crate) struct Recordings {
    items: Mutex<Vec<Stored>>,
    /// Poll for notices until then (a recording of ours ended).
    poll_until: Mutex<Option<Instant>>,
}

fn parse_action(a: &Value) -> Option<Action> {
    Some(Action {
        method: a["type"].as_str()?.to_string(),
        link: a["link"].as_str().filter(|l| !l.is_empty())?.to_string(),
    })
}

/// Seconds since the epoch from the `timestamp` query parameter of an action link, which
/// the server sets to the notification time.
fn link_timestamp(link: &str) -> Option<i64> {
    let query = link.split_once('?')?.1;
    query.split('&').find_map(|kv| kv.strip_prefix("timestamp=")?.parse().ok())
}

fn parse_notices(list: &[Value]) -> Vec<Stored> {
    list.iter()
        .filter(|n| n["app"] == "spreed")
        .filter_map(|n| {
            let object_type = n["object_type"].as_str()?;
            if object_type != "recording" && object_type != "recording_information" {
                return None;
            }
            let id = n["notification_id"].as_i64()?;
            let token = n["object_id"].as_str()?.to_string();
            let params = if n["messageRichParameters"].is_object() {
                &n["messageRichParameters"]
            } else {
                &n["subjectRichParameters"]
            };
            let room_name = params["call"]["name"].as_str().unwrap_or_default().to_string();
            let file_name = params["file"]["name"].as_str().unwrap_or_default().to_string();
            let actions: Vec<&Value> = n["actions"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
            let share = actions.iter().find(|a| a["primary"] == true).and_then(|a| parse_action(a));
            let dismiss = actions.iter().find(|a| a["primary"] != true).and_then(|a| parse_action(a));
            // The raw subject is not in the API: tell the kinds apart by object type,
            // actions and file name (as the server does in `shareToChat`).
            let summary = file_name.ends_with(" - summary.md");
            let markdown = file_name.ends_with(".md");
            let kind = if object_type == "recording_information" {
                RecordingNoticeKind::RecordingFailed
            } else if share.is_none() {
                if summary { RecordingNoticeKind::SummaryFailed } else { RecordingNoticeKind::TranscriptFailed }
            } else if summary {
                RecordingNoticeKind::Summary
            } else if markdown {
                RecordingNoticeKind::Transcript
            } else {
                RecordingNoticeKind::Recording
            };
            let timestamp = share
                .as_ref()
                .or(dismiss.as_ref())
                .and_then(|a| link_timestamp(&a.link))
                .unwrap_or(0);
            Some(Stored {
                notice: RecordingNotice {
                    id,
                    token,
                    room_name,
                    kind,
                    file_name,
                    timestamp,
                    can_share: share.is_some(),
                },
                share,
                dismiss,
            })
        })
        .collect()
}

/// Readable message for a failed start/stop (server answers 400 `{error: …}`).
fn recording_error(e: Error) -> Error {
    let Error::Http { status: 400, message } = &e else { return e };
    let code = serde_json::from_str::<Value>(message)
        .ok()
        .and_then(|v| v["ocs"]["data"]["error"].as_str().map(str::to_string))
        .unwrap_or_default();
    let text = match code.as_str() {
        "recording" => "Die Aufnahme läuft bereits",
        "call" => "Es läuft kein Anruf",
        "config" => "Aufnahmen sind auf dem Server nicht eingerichtet",
        _ => "Die Aufnahme konnte nicht gestartet werden",
    };
    Error::Call {
        code: format!("recording-{}", if code.is_empty() { "failed" } else { code.as_str() }),
        message: text.into(),
    }
}

impl Recordings {
    /// Replaces the list from a fresh notifications response; `true` if it changed.
    fn update(&self, list: &[Value]) -> bool {
        let new = parse_notices(list);
        let mut items = self.items.lock().unwrap();
        if *items == new {
            return false;
        }
        *items = new;
        true
    }

    fn notices(&self) -> Vec<RecordingNotice> {
        self.items.lock().unwrap().iter().map(|s| s.notice.clone()).collect()
    }

    fn take(&self, id: i64) -> Option<Stored> {
        let mut items = self.items.lock().unwrap();
        let i = items.iter().position(|s| s.notice.id == id)?;
        Some(items.remove(i))
    }
}

impl Engine {
    /// Starts recording the current call (moderators, see `CallView::can_record`).
    /// `video`: records the call view (avatars, screen shares); otherwise audio only.
    pub async fn start_recording(&self, video: bool) -> Result<()> {
        let call = self.current_call().filter(|c| c.can_record).ok_or_else(|| Error::Call {
            code: "recording-not-allowed".into(),
            message: "Aufnehmen ist in diesem Anruf nicht möglich".into(),
        })?;
        self.client.start_recording(&call.token, video).await.map_err(recording_error)?;
        tracing::info!(token = %call.token, video, "recording started");
        self.calls.recording_requested(if video { CallRecording::StartingVideo } else { CallRecording::StartingAudio });
        // The server stops the recording when the call ends; the notice comes to us.
        if let Some(rt) = self.runtime.get() {
            let weak = self.weak_self();
            let id = call.id;
            rt.spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let Some(e) = weak.upgrade() else { return };
                    if e.current_call().is_none_or(|c| c.id != id) {
                        e.poll_recording_notices();
                        return;
                    }
                }
            });
        }
        Ok(())
    }

    /// Stops the recording of the current call. The file arrives later as a
    /// [`RecordingNotice`] for whoever started it.
    pub async fn stop_recording(&self) -> Result<()> {
        let call = self.current_call().filter(|c| c.can_record).ok_or_else(|| Error::Call {
            code: "recording-not-allowed".into(),
            message: "Aufnehmen ist in diesem Anruf nicht möglich".into(),
        })?;
        self.client.stop_recording(&call.token).await.map_err(recording_error)?;
        tracing::info!(token = %call.token, "recording stopped");
        self.calls.recording_requested(CallRecording::Off);
        self.poll_recording_notices();
        Ok(())
    }

    /// Finished recordings (and transcripts, summaries, failures) waiting for the user.
    /// Changes arrive as [`Event::RecordingsChanged`].
    pub fn recording_notices(&self) -> Vec<RecordingNotice> {
        self.recordings.notices()
    }

    /// Posts the file of a notice into its conversation ("In Chat teilen"); the notice
    /// is removed.
    pub async fn share_recording(&self, id: i64) -> Result<()> {
        let item = self.recordings.items.lock().unwrap().iter().find(|s| s.notice.id == id).cloned();
        let Some(Stored { share: Some(a), .. }) = item else {
            return Err(Error::Call { code: "recording-notice".into(), message: "Nicht mehr verfügbar".into() });
        };
        self.client.notification_action(&a.method, &a.link).await?;
        self.recordings.take(id);
        self.emit_recordings();
        Ok(())
    }

    /// Discards a notice ("Verwerfen"); the file stays in the user's files.
    pub async fn dismiss_recording(&self, id: i64) -> Result<()> {
        let Some(item) = self.recordings.take(id) else { return Ok(()) };
        let r = match &item.dismiss {
            Some(a) => self.client.notification_action(&a.method, &a.link).await,
            None => self.client.dismiss_notification(id).await,
        };
        if let Err(e) = r {
            if !matches!(e, Error::NotFound) {
                self.recordings.items.lock().unwrap().push(item);
                return Err(e);
            }
        }
        self.emit_recordings();
        Ok(())
    }

    fn emit_recordings(&self) {
        self.sink.emit(Event::RecordingsChanged { items: self.recordings.notices() });
    }

    /// Updates the notices from a notifications response (see `check_incoming_calls`).
    pub(crate) fn update_recording_notices(&self, list: &[Value]) {
        if self.recordings.update(list) {
            self.emit_recordings();
        }
    }

    /// A recording of ours ended: look for its notice regularly for a while, in case
    /// notify_push does not tell us.
    pub(crate) fn poll_recording_notices(&self) {
        let Some(rt) = self.runtime.get() else { return };
        {
            let mut until = self.recordings.poll_until.lock().unwrap();
            let running = until.is_some_and(|u| u > Instant::now());
            *until = Some(Instant::now() + NOTICE_POLL_FOR);
            if running {
                return;
            }
        }
        let weak = self.weak_self();
        rt.spawn(async move {
            loop {
                tokio::time::sleep(NOTICE_POLL).await;
                let Some(e) = weak.upgrade() else { return };
                if !e.recordings.poll_until.lock().unwrap().is_some_and(|u| u > Instant::now()) {
                    return;
                }
                e.check_incoming_calls().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stored(subject_file: &str, object_type: &str, with_actions: bool) -> Value {
        let base = "https://cloud.example/ocs/v2.php/apps/spreed/api/v1/recording/tok";
        let actions = if with_actions {
            json!([
                {"label":"Share to chat","link":format!("{base}/share-chat?fileId=5&timestamp=1700000000"),"type":"POST","primary":true},
                {"label":"Dismiss notification","link":format!("{base}/notification?timestamp=1700000000"),"type":"DELETE","primary":false}
            ])
        } else {
            json!([])
        };
        json!({
            "notification_id": 42, "app": "spreed", "object_type": object_type, "object_id": "tok",
            "subject": "Call recording now available",
            "messageRichParameters": {
                "call": {"type":"call","id":"1","name":"Team"},
                "file": {"type":"file","id":"5","name":subject_file,"path":"Talk/Recording/tok/x"}
            },
            "actions": actions
        })
    }

    #[test]
    fn parses_recording_notices() {
        let list = vec![
            json!({"app":"spreed","object_type":"call","object_id":"tok","notification_id":1}),
            stored("Recording 2026-11-11.ogg", "recording", true),
            stored("Recording 2026-11-11.md", "recording", true),
            stored("Recording 2026-11-11 - summary.md", "recording", true),
            stored("Recording 2026-11-11.md", "recording", false),
            json!({"notification_id": 43, "app": "spreed", "object_type": "recording_information", "object_id": "tok",
                   "messageRichParameters": {"call": {"name": "Team"}}, "actions": []}),
        ];
        let n = parse_notices(&list);
        let kinds: Vec<_> = n.iter().map(|s| s.notice.kind).collect();
        assert_eq!(
            kinds,
            [
                RecordingNoticeKind::Recording,
                RecordingNoticeKind::Transcript,
                RecordingNoticeKind::Summary,
                RecordingNoticeKind::TranscriptFailed,
                RecordingNoticeKind::RecordingFailed,
            ]
        );
        let r = &n[0];
        assert_eq!((r.notice.id, r.notice.token.as_str(), r.notice.room_name.as_str()), (42, "tok", "Team"));
        assert_eq!(r.notice.timestamp, 1_700_000_000);
        assert!(r.notice.can_share);
        assert_eq!(r.share.as_ref().unwrap().method, "POST");
        assert_eq!(r.dismiss.as_ref().unwrap().method, "DELETE");
        assert!(!n[4].notice.can_share && n[4].dismiss.is_none());
    }

    #[test]
    fn readable_recording_errors() {
        let e = recording_error(Error::Http {
            status: 400,
            message: r#"{"ocs":{"meta":{"status":"failure"},"data":{"error":"recording"}}}"#.into(),
        });
        assert!(matches!(e, Error::Call { ref code, .. } if code == "recording-recording"));
        assert!(matches!(recording_error(Error::NotFound), Error::NotFound));
    }

    #[test]
    fn server_values() {
        assert_eq!(CallRecording::from_server(4), CallRecording::StartingAudio);
        assert_eq!(CallRecording::from_server(1), CallRecording::Video);
        assert_eq!(CallRecording::from_server(9), CallRecording::Off);
        assert!(CallRecording::StartingVideo.is_active() && !CallRecording::Failed.is_active());
    }
}
