//! Media engine on headless Chrome (real libwebrtc, like the apps): the core's commands
//! go to a page over the DevTools protocol, the page reports back through a binding.
//! The microphone is a generated sine tone (`--use-file-for-fake-audio-capture`), the
//! camera Chrome's fake device (moving test pattern).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use talk_core::{Engine, IceCandidate, IceServer, MediaEngine, MediaReport, PeerKind, SdpType};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

use crate::callbot::RxStats;

const PAGE_JS: &str = r#"
const peers = new Map(), queues = new Map(), stats = {};
let mic = null, micEnabled = true, cam = null;
window.talkStats = stats;
const report = (o) => window.talkReport(JSON.stringify(o));
async function getMic() {
  if (!mic) mic = await navigator.mediaDevices.getUserMedia({audio: {echoCancellation: false, noiseSuppression: false, autoGainControl: false}});
  return mic;
}
function enqueue(id, fn) {
  const q = (queues.get(id) || Promise.resolve()).then(fn).catch((e) => report({type: 'peerError', peerId: id, message: String(e)}));
  queues.set(id, q);
}
window.talkCmd = (c) => {
  const id = c.peerId;
  switch (c.cmd) {
  case 'createPeer': enqueue(id, async () => {
    const pc = new RTCPeerConnection({bundlePolicy: 'max-bundle', rtcpMuxPolicy: 'require',
      iceServers: c.iceServers.map((s) => ({urls: s.urls, username: s.username || undefined, credential: s.credential || undefined}))});
    peers.set(id, pc);
    pc.onicecandidate = (e) => { if (e.candidate && e.candidate.candidate)
      report({type: 'localCandidate', peerId: id, candidate: {candidate: e.candidate.candidate, sdpMid: e.candidate.sdpMid, sdpMLineIndex: e.candidate.sdpMLineIndex ?? 0}}); };
    pc.onconnectionstatechange = () => report({type: 'peerState', peerId: id, state: pc.connectionState});
    pc.ontrack = (e) => { if (e.track.kind !== 'audio') return; const a = new Audio(); a.srcObject = e.streams[0] || new MediaStream([e.track]); a.play().catch(() => {}); };
    if (c.kind !== 'subscriber') {
      const m = await getMic(); const t = m.getAudioTracks()[0]; t.enabled = micEnabled;
      const dir = c.kind === 'publisher' ? 'sendonly' : 'sendrecv';
      pc.addTransceiver(t, {direction: dir, streams: [m]});
      // Always a video transceiver; the camera track is attached when switched on.
      pc.addTransceiver(cam ? cam.getVideoTracks()[0] : 'video', {direction: dir, streams: [m]});
    }
  }); break;
  case 'createOffer': enqueue(id, async () => { const pc = peers.get(id); if (!pc) return;
    await pc.setLocalDescription(await pc.createOffer({iceRestart: c.iceRestart}));
    report({type: 'localDescription', peerId: id, sdpType: 'offer', sdp: pc.localDescription.sdp}); }); break;
  case 'createAnswer': enqueue(id, async () => { const pc = peers.get(id); if (!pc) return;
    await pc.setLocalDescription(await pc.createAnswer());
    report({type: 'localDescription', peerId: id, sdpType: 'answer', sdp: pc.localDescription.sdp}); }); break;
  case 'setRemoteDescription': enqueue(id, async () => { const pc = peers.get(id); if (pc) await pc.setRemoteDescription({type: c.sdpType, sdp: c.sdp}); }); break;
  case 'addRemoteCandidate': enqueue(id, async () => { const pc = peers.get(id); if (pc) await pc.addIceCandidate(c.candidate).catch((e) => console.log('candidate', e)); }); break;
  case 'closePeer': enqueue(id, async () => { const pc = peers.get(id); if (pc) { pc.close(); peers.delete(id); } }); break;
  case 'setMicrophoneEnabled': micEnabled = c.enabled; if (mic) mic.getAudioTracks().forEach((t) => t.enabled = c.enabled); break;
  case 'setCameraEnabled': (async () => {
    try {
      if (c.enabled && !cam) cam = await navigator.mediaDevices.getUserMedia({video: {width: 640, height: 480, frameRate: 24}});
      if (!c.enabled && cam) { cam.getTracks().forEach((t) => t.stop()); cam = null; }
      const track = cam ? cam.getVideoTracks()[0] : null;
      for (const pc of peers.values()) for (const tr of pc.getTransceivers())
        if (tr.receiver.track.kind === 'video' && (tr.direction === 'sendonly' || tr.direction === 'sendrecv')) await tr.sender.replaceTrack(track);
    } catch (e) { console.log('camera', e); report({type: 'cameraState', on: false}); }
  })(); break;
  case 'stopAudio':
    if (mic) { mic.getTracks().forEach((t) => t.stop()); mic = null; }
    if (cam) { cam.getTracks().forEach((t) => t.stop()); cam = null; }
    break;
  }
};
setInterval(async () => {
  for (const [id, pc] of peers) {
    const r = await pc.getStats();
    r.forEach((s) => {
      if (s.type === 'inbound-rtp' && s.kind === 'video') {
        const st = stats[id] || (stats[id] = {packets: 0, max: 0});
        st.frames = s.framesDecoded || 0; st.width = s.frameWidth || 0; st.height = s.frameHeight || 0;
      }
      if (s.type === 'inbound-rtp' && s.kind === 'audio') {
        const st = stats[id] || (stats[id] = {packets: 0, max: 0});
        st.packets = s.packetsReceived || 0;
        const lv = s.audioLevel || 0; st.max = Math.max(st.max, lv);
        report({type: 'audioLevel', peerId: id, level: lv});
      }
    });
  }
}, 300);
"#;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

pub struct ChromeMedia {
    engine: OnceLock<Weak<Engine>>,
    out: mpsc::UnboundedSender<(String, Option<oneshot::Sender<Value>>)>,
    _child: tokio::process::Child,
    _dir: PathBuf,
}

/// 48 kHz mono 16-bit sine, 10 s (Chrome loops the file).
fn write_tone(path: &std::path::Path, hz: f32) -> Result<()> {
    let rate = 48000u32;
    let n = rate * 10;
    let mut data = Vec::with_capacity(44 + n as usize * 2);
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + n * 2).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&rate.to_le_bytes());
    data.extend_from_slice(&(rate * 2).to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        let v = (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin() * 0.3;
        data.extend_from_slice(&((v * i16::MAX as f32) as i16).to_le_bytes());
    }
    std::fs::write(path, data)?;
    Ok(())
}

fn chrome_binary() -> String {
    std::env::var("CHROME").unwrap_or_else(|_| "google-chrome".into())
}

impl ChromeMedia {
    pub async fn launch(tone_hz: f32) -> Result<Arc<Self>> {
        let dir = std::env::temp_dir().join(format!("talk-chromebot-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let wav = dir.join("tone.wav");
        write_tone(&wav, tone_hz)?;
        let page = dir.join("bot.html");
        std::fs::write(&page, "<!doctype html><title>talk bot</title>")?;
        let mut cmd = tokio::process::Command::new(chrome_binary());
        // Chrome must not outlive us: kill_on_drop only helps when we exit cleanly.
        #[cfg(target_os = "linux")]
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                // We may have died between fork and prctl.
                if libc::getppid() == 1 {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
        let mut child = cmd
            .args([
                "--headless=new",
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
                "--use-fake-ui-for-media-stream",
                "--use-fake-device-for-media-stream",
                "--autoplay-policy=no-user-gesture-required",
                "--disable-background-timer-throttling",
                "--disable-renderer-backgrounding",
            ])
            .arg(format!("--use-file-for-fake-audio-capture={}", wav.display()))
            .arg(format!("--user-data-dir={}", dir.join("profile").display()))
            .arg(format!("file://{}", page.display()))
            .stderr(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("Chrome starten (CHROME=… setzen, falls nicht google-chrome)")?;
        // "DevTools listening on ws://127.0.0.1:PORT/devtools/browser/…"
        let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
        let port = tokio::time::timeout(Duration::from_secs(15), async {
            while let Some(l) = lines.next_line().await? {
                if let Some(rest) = l.split("ws://127.0.0.1:").nth(1) {
                    return Ok::<_, anyhow::Error>(rest.split('/').next().unwrap_or_default().to_string());
                }
            }
            bail!("Chrome ohne DevTools-Port beendet")
        })
        .await
        .context("Chrome startet nicht")??;
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });

        // The page target.
        let list: Vec<Value> = reqwest::get(format!("http://127.0.0.1:{port}/json/list")).await?.json().await?;
        let ws_url = list
            .iter()
            .find(|t| t["type"] == "page")
            .and_then(|t| t["webSocketDebuggerUrl"].as_str())
            .context("keine Seite in Chrome")?
            .to_string();
        let (ws, _) = tokio_tungstenite::connect_async(&ws_url).await?;
        let (mut sink, mut stream) = ws.split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<(String, Option<oneshot::Sender<Value>>)>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let me = Arc::new(Self {
            engine: OnceLock::new(),
            out: out_tx,
            _child: child,
            _dir: dir,
        });

        // Writer: Runtime.evaluate per expression, in order.
        let p = pending.clone();
        tokio::spawn(async move {
            let mut id = 100u64;
            let _ = sink
                .send(Message::text(json!({"id": 1, "method": "Runtime.enable"}).to_string()))
                .await;
            let _ = sink
                .send(Message::text(
                    json!({"id": 2, "method": "Runtime.addBinding", "params": {"name": "talkReport"}}).to_string(),
                ))
                .await;
            let _ = sink
                .send(Message::text(
                    json!({"id": 3, "method": "Runtime.evaluate", "params": {"expression": PAGE_JS}}).to_string(),
                ))
                .await;
            while let Some((expr, reply)) = out_rx.recv().await {
                id += 1;
                if let Some(r) = reply {
                    p.lock().unwrap().insert(id, r);
                }
                let msg = json!({"id": id, "method": "Runtime.evaluate",
                    "params": {"expression": expr, "returnByValue": true, "awaitPromise": true}});
                if sink.send(Message::text(msg.to_string())).await.is_err() {
                    return;
                }
            }
        });

        // Reader: binding calls → core reports; evaluate results → waiting callers.
        let weak = Arc::downgrade(&me);
        let p = pending.clone();
        tokio::spawn(async move {
            while let Some(Ok(m)) = stream.next().await {
                let Message::Text(t) = m else { continue };
                let Ok(v) = serde_json::from_str::<Value>(&t) else { continue };
                if v["method"] == "Runtime.bindingCalled" {
                    let payload = v["params"]["payload"].as_str().unwrap_or_default();
                    match serde_json::from_str::<MediaReport>(payload) {
                        Ok(r) => {
                            if let Some(e) = weak.upgrade().and_then(|m| m.engine.get().and_then(Weak::upgrade)) {
                                e.media_report(r);
                            }
                        }
                        Err(e) => tracing::warn!("chrome report {payload}: {e}"),
                    }
                } else if v["method"] == "Runtime.consoleAPICalled" {
                    tracing::debug!("chrome console: {}", v["params"]["args"]);
                } else if v["method"] == "Runtime.exceptionThrown" {
                    tracing::warn!("chrome exception: {}", v["params"]["exceptionDetails"]);
                } else if let Some(id) = v["id"].as_u64() {
                    if let Some(tx) = p.lock().unwrap().remove(&id) {
                        let _ = tx.send(v["result"]["result"]["value"].clone());
                    }
                }
            }
        });
        // Wait until the page script is installed.
        let ready = me.eval_value("typeof window.talkCmd").await?;
        if ready != "function" {
            bail!("Seiten-Skript nicht geladen: {ready}");
        }
        Ok(me)
    }

    pub fn attach(&self, engine: &Arc<Engine>) {
        let _ = self.engine.set(Arc::downgrade(engine));
    }

    fn cmd(&self, v: Value) {
        let _ = self.out.send((format!("talkCmd({v})"), None));
    }

    async fn eval_value(&self, expr: &str) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        self.out.send((expr.to_string(), Some(tx)))?;
        Ok(tokio::time::timeout(Duration::from_secs(10), rx).await??)
    }

    pub async fn stats(&self) -> HashMap<String, RxStats> {
        let v = self
            .eval_value("JSON.stringify(window.talkStats)")
            .await
            .ok()
            .and_then(|v| v.as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()))
            .unwrap_or_default();
        v.as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, s)| {
                        (
                            k.clone(),
                            RxStats {
                                packets: s["packets"].as_u64().unwrap_or(0),
                                max_level: s["max"].as_f64().unwrap_or(0.0) as f32,
                                frames: s["frames"].as_u64().unwrap_or(0),
                                width: s["width"].as_u64().unwrap_or(0) as u32,
                                height: s["height"].as_u64().unwrap_or(0) as u32,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl MediaEngine for ChromeMedia {
    fn create_peer(&self, peer_id: String, kind: PeerKind, ice_servers: Vec<IceServer>) {
        self.cmd(json!({"cmd": "createPeer", "peerId": peer_id, "kind": kind, "iceServers": ice_servers}));
    }
    fn create_offer(&self, peer_id: String, ice_restart: bool) {
        self.cmd(json!({"cmd": "createOffer", "peerId": peer_id, "iceRestart": ice_restart}));
    }
    fn create_answer(&self, peer_id: String) {
        self.cmd(json!({"cmd": "createAnswer", "peerId": peer_id}));
    }
    fn set_remote_description(&self, peer_id: String, sdp_type: SdpType, sdp: String) {
        self.cmd(json!({"cmd": "setRemoteDescription", "peerId": peer_id, "sdpType": sdp_type, "sdp": sdp}));
    }
    fn add_remote_candidate(&self, peer_id: String, candidate: IceCandidate) {
        self.cmd(json!({"cmd": "addRemoteCandidate", "peerId": peer_id, "candidate": candidate}));
    }
    fn close_peer(&self, peer_id: String) {
        self.cmd(json!({"cmd": "closePeer", "peerId": peer_id}));
    }
    fn set_microphone_enabled(&self, enabled: bool) {
        self.cmd(json!({"cmd": "setMicrophoneEnabled", "enabled": enabled}));
    }
    fn set_camera_enabled(&self, enabled: bool) {
        self.cmd(json!({"cmd": "setCameraEnabled", "enabled": enabled}));
    }
    fn start_audio(&self, _call_id: String) {}
    fn stop_audio(&self, _call_id: String) {
        self.cmd(json!({"cmd": "stopAudio"}));
    }
}
