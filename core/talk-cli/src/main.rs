//! `talk-cli` – exercise the shared core against a real server.
//!
//! Credentials come from a `.env` file (searched upwards from the current directory):
//! `NC_URL` plus `NC_USER/NC_PASS` (account `me`), `BOT_A_*` (`a`), `BOT_B_*` (`b`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[cfg(feature = "call-bot")]
mod callbot;
#[cfg(feature = "call-bot")]
mod chromebot;
use talk_core::{
    Account, ChatMessage, Engine, Event, EventSink, LoginFlow, Platform, TalkClient,
    new_reference_id,
};

#[derive(Parser)]
#[command(version, about = "Talk core test CLI")]
struct Cli {
    /// Account from .env: me | a | b
    #[arg(short, long, default_value = "me", global = true)]
    account: String,
    /// Path to .env (default: search upwards from the current directory)
    #[arg(long, global = true)]
    env_file: Option<PathBuf>,
    /// Outbox directory (default: ~/.cache/talk-cli/<account>)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Login Flow v2 in the browser; prints the resulting app password
    Login {
        /// e.g. cloud.example.org
        server: String,
    },
    /// List conversations (sorted by last activity)
    Rooms {
        #[arg(long)]
        json: bool,
    },
    /// Send a text message
    Send { token: String, text: String },
    /// Upload a file to the Talk attachment folder and share it into a conversation
    SendFile {
        token: String,
        path: String,
        /// Caption shown with the file
        #[arg(short, long)]
        caption: Option<String>,
    },
    /// Print recent messages, then follow new ones via long polling
    Tail {
        token: String,
        #[arg(short = 'n', long, default_value_t = 20)]
        lines: u32,
        /// Exit after printing history
        #[arg(long)]
        no_follow: bool,
    },
    /// Show signaling server, STUN/TURN (credentials redacted)
    Signaling,
    /// Send through the persistent outbox (retries, resumes after a kill) and wait until
    /// delivered. Fault injection: TALK_FAULTS=put,chunk2,move,share,send
    Enqueue {
        token: String,
        /// Text message (or caption with --file)
        #[arg(short, long)]
        text: Option<String>,
        /// File to send (copied into the outbox first)
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Send the file as voice message (Talk accepts audio/mpeg and audio/wav)
        #[arg(long)]
        voice: bool,
    },
    /// Resume the outbox of an earlier (killed) run until everything is delivered
    OutboxRun,
    /// Record a voice message from an audio file (decoded with ffmpeg, pushed in 100 ms
    /// blocks like a microphone) and send it through the outbox
    Voice {
        token: String,
        input: PathBuf,
        /// Simulate a kill: exit abruptly after this many seconds of audio (draft stays)
        #[arg(long)]
        abort_after: Option<u64>,
        /// Keep the recording as draft instead of sending it
        #[arg(long)]
        draft: bool,
    },
    /// Show the voice draft of a conversation (recovers an interrupted one); --send sends it
    VoiceDraft {
        token: String,
        #[arg(long)]
        send: bool,
    },
    /// Duration and waveform of an audio file (as the apps compute it)
    AudioInfo { path: PathBuf },
    /// Download a shared file like the apps do (resumable), print the local path
    Fetch { path: String, size: i64 },
    /// Call test peer: call (or answer with --answer), send a tone, check received audio,
    /// hang up and verify the server shows us out of the call. Build with --features call-bot
    Call {
        token: String,
        /// Wait for an incoming call in `token` and answer it
        #[arg(long)]
        answer: bool,
        /// Do not ring anybody (shared test rooms)
        #[arg(long)]
        silent: bool,
        /// How long to stay connected
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        /// Fail unless audio from the other side arrived
        #[arg(long)]
        expect_audio: bool,
        /// Mute after this many seconds
        #[arg(long)]
        mute_after: Option<u64>,
        /// Tone frequency in Hz
        #[arg(long, default_value_t = 440.0)]
        tone: f32,
        /// With --answer: how long to wait for the call
        #[arg(long, default_value_t = 120)]
        ring_wait: u64,
        /// With --answer: no polling, only the core's own detection (notify_push, room list)
        #[arg(long)]
        passive: bool,
        /// Hang up when not connected after this many seconds
        #[arg(long, default_value_t = 90)]
        connect_timeout: u64,
        /// Print every call change as JSON on stdout
        #[arg(long)]
        json: bool,
    },
    /// Cache check: shows the cached state (timed), then syncs a chat and shows the delta
    Cache {
        token: Option<String>,
        /// Number of cached messages to print
        #[arg(short = 'n', long, default_value_t = 10)]
        lines: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "talk_core=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Cmd::Login { server } = &cli.cmd {
        let flow = LoginFlow::start(server, Platform::Cli).await?;
        eprintln!("Im Browser anmelden:\n  {}\n", flow.login_url);
        let _ = std::process::Command::new("xdg-open")
            .arg(&flow.login_url)
            .spawn();
        let creds = flow.wait().await?;
        println!(
            "server={}\nlogin={}\napp_password={}",
            creds.server, creds.login_name, creds.app_password
        );
        return Ok(());
    }

    let client = TalkClient::new(load_account(&cli)?, Platform::Cli)?;

    if let Cmd::AudioInfo { path } = &cli.cmd {
        let t = Instant::now();
        let info = talk_core::voice::analyze(path)?;
        println!("Dauer {} ms (analysiert in {:?})", info.duration_ms, t.elapsed());
        println!("Wellenform {:?}", info.waveform);
        return Ok(());
    }
    if let Cmd::Enqueue { .. } | Cmd::OutboxRun | Cmd::Voice { .. } | Cmd::VoiceDraft { .. } | Cmd::Fetch { .. } =
        &cli.cmd
    {
        return run_outbox(&cli, client).await;
    }
    if let Cmd::Call { .. } = &cli.cmd {
        return call_cmd(&cli).await;
    }
    if let Cmd::Cache { token, lines } = &cli.cmd {
        return run_cache(&cli, client, token.as_deref(), *lines).await;
    }

    match cli.cmd {
        Cmd::Login { .. } | Cmd::Enqueue { .. } | Cmd::OutboxRun | Cmd::Cache { .. } | Cmd::Call { .. }
        | Cmd::Voice { .. }
        | Cmd::VoiceDraft { .. }
        | Cmd::AudioInfo { .. }
        | Cmd::Fetch { .. } => unreachable!(),
        Cmd::Rooms { json } => {
            let t = Instant::now();
            let mut batch = client.rooms(None, true).await?;
            let elapsed = t.elapsed();
            if json {
                println!("{}", serde_json::to_string_pretty(&batch.rooms)?);
                return Ok(());
            }
            batch
                .rooms
                .sort_by_key(|r| std::cmp::Reverse(r.last_activity));
            for r in &batch.rooms {
                let preview = r
                    .last_message
                    .as_ref()
                    .map(|m| {
                        format!(
                            "{}: {}",
                            m.actor_display_name,
                            one_line(&m.rendered_text(), 50)
                        )
                    })
                    .unwrap_or_default();
                let unread = if r.unread_messages > 0 {
                    format!("({})", r.unread_messages)
                } else {
                    String::new()
                };
                println!(
                    "{:<10} {:<5} {:<30} {}",
                    r.token,
                    unread,
                    one_line(&r.display_name, 30),
                    preview
                );
            }
            eprintln!("{} Gespräche in {:?}", batch.rooms.len(), elapsed);
        }
        Cmd::Send { token, text } => {
            let t = Instant::now();
            let msg = client
                .send_message(&token, &text, &new_reference_id(), None)
                .await?;
            eprintln!("gesendet: id={} in {:?}", msg.id, t.elapsed());
        }
        Cmd::SendFile {
            token,
            path,
            caption,
        } => {
            let t = Instant::now();
            let user = client.current_user().await?;
            let folder = client.attachment_folder().await?;
            let name = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            let remote = client
                .upload(&user.id, &path, &folder, &name, "application/octet-stream")
                .await?;
            client
                .share_to_room(&token, &remote, &new_reference_id(), caption.as_deref(), None, false)
                .await?;
            eprintln!("gesendet: {remote} in {:?}", t.elapsed());
        }
        Cmd::Tail {
            token,
            lines,
            no_follow,
        } => {
            let history = client.history(&token, None, lines).await?;
            let mut last = history.messages.last().map(|m| m.id).unwrap_or(0);
            history.messages.iter().for_each(print_message);
            if no_follow {
                return Ok(());
            }
            loop {
                let batch = client.poll_messages(&token, last, 30).await?;
                batch.messages.iter().for_each(print_message);
                if let Some(id) = batch.last_given.or(batch.messages.last().map(|m| m.id)) {
                    last = last.max(id);
                }
            }
        }
        Cmd::Signaling => {
            let s = client.signaling_settings(None).await?;
            println!("mode:   {}", s.signaling_mode);
            println!("server: {}", s.server);
            println!(
                "hello v2 token: {}",
                s.hello_auth_params.get("2.0").is_some()
            );
            for ice in s.stunservers.iter().chain(&s.turnservers) {
                println!(
                    "ice:    {:?} user={}",
                    ice.urls,
                    ice.username.as_deref().unwrap_or("-")
                );
            }
        }
    }
    Ok(())
}

#[cfg(feature = "call-bot")]
async fn call_cmd(cli: &Cli) -> Result<()> {
    let Cmd::Call { token, answer, silent, seconds, expect_audio, mute_after, tone, ring_wait, connect_timeout, passive, json } = &cli.cmd else {
        unreachable!()
    };
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let account = load_account(cli)?;
    let client = TalkClient::new(account, Platform::Cli)?;
    let sink = Arc::new(callbot::CallSink {
        latest: Mutex::new(None),
        changed: tokio::sync::Notify::new(),
        json: *json,
    });
    let engine = Engine::new(client, sink.clone(), None);
    let media = if std::env::var("BOT_MEDIA").as_deref() == Ok("rust") {
        let m = callbot::BotMedia::new(*tone)?;
        m.attach(&engine);
        engine.set_media_engine(m.clone());
        callbot::Media::Rust(m)
    } else {
        let m = chromebot::ChromeMedia::launch(*tone).await?;
        m.attach(&engine);
        engine.set_media_engine(m.clone());
        callbot::Media::Chrome(m)
    };
    engine.start().await?;
    let opts = callbot::CallOpts {
        token: token.clone(),
        answer: *answer,
        silent: *silent,
        seconds: *seconds,
        expect_audio: *expect_audio,
        mute_after: *mute_after,
        ring_wait: *ring_wait,
        connect_timeout: *connect_timeout,
        passive: *passive,
    };
    let r = callbot::run(engine.clone(), sink, media, opts).await;
    engine.shutdown();
    r
}

#[cfg(not(feature = "call-bot"))]
async fn call_cmd(_cli: &Cli) -> Result<()> {
    bail!("talk-cli ohne Anruf-Bot gebaut: cargo run -p talk-cli --features call-bot -- call …")
}

/// Prints outbox state changes (deduplicated per message).
struct PrintSink {
    last: Mutex<HashMap<String, String>>,
}

impl EventSink for PrintSink {
    fn emit(&self, event: Event) {
        let Event::OutboxChanged { items, .. } = event else { return };
        let mut last = self.last.lock().unwrap();
        for p in items {
            let r = &p.message.reference_id[..8];
            let what = match &p.message.attachment {
                Some(a) => format!("Datei {}", a.name),
                None => format!("Text \"{}\"", one_line(&p.message.text, 30)),
            };
            let progress = if p.progress >= 0.0 {
                format!(" {:>3.0} %", p.progress * 100.0)
            } else {
                String::new()
            };
            let line = format!("{:?}{progress}{}", p.state, p.error.map(|e| format!(" {e:?}")).unwrap_or_default());
            if last.get(r) != Some(&line) {
                eprintln!("[{r}] {what}: {line}");
                last.insert(r.to_string(), line);
            }
        }
    }
}

fn data_dir(cli: &Cli) -> Result<PathBuf> {
    Ok(match &cli.data_dir {
        Some(d) => d.clone(),
        None => PathBuf::from(std::env::var("HOME")?)
            .join(".cache/talk-cli")
            .join(&cli.account),
    })
}

async fn run_cache(cli: &Cli, client: TalkClient, token: Option<&str>, lines: usize) -> Result<()> {
    let t = Instant::now();
    let engine = Engine::new(
        client,
        Arc::new(PrintSink {
            last: Mutex::default(),
        }),
        Some(data_dir(cli)?),
    );
    let rooms = engine.rooms();
    eprintln!("Cache: {} Räume in {:?} (ohne Netz)", rooms.len(), t.elapsed());
    let show = |label: &str, page: &talk_core::ChatPage| {
        eprintln!(
            "{label}: {} sichtbare Nachrichten, älteste id {:?}, mehr: {}, reset: {}",
            page.messages.len(),
            page.oldest_id,
            page.has_more,
            page.reset
        );
        for m in page.messages.iter().rev().take(lines).rev() {
            let edited = if m.edited { " (bearbeitet)" } else { "" };
            let reactions: String = m.reactions.iter().map(|(k, v)| format!(" {k}{v}")).collect();
            println!("  [{}] {} {}: {}{edited}{reactions}", m.id, m.kind, m.author, one_line(&m.text, 60));
        }
    };
    let Some(token) = token else {
        for r in rooms.iter().take(lines) {
            println!("  {} {}", r.token, r.name);
        }
        return Ok(());
    };
    let t = Instant::now();
    let cached = engine.cached_chat(token);
    eprintln!("cached_chat in {:?}", t.elapsed());
    show("Cache", &cached);
    if let Err(e) = engine.start().await {
        eprintln!("Start: {e}");
    }
    let t = Instant::now();
    let page = engine.open_chat(token).await?;
    eprintln!("open_chat (Delta-Sync) in {:?}", t.elapsed());
    show("Server", &page);
    engine.shutdown();
    Ok(())
}

async fn run_outbox(cli: &Cli, client: TalkClient) -> Result<()> {
    let dir = data_dir(cli)?;
    let engine = Engine::new(
        client,
        Arc::new(PrintSink {
            last: Mutex::default(),
        }),
        Some(dir.clone()),
    );
    let t = Instant::now();
    if let Err(e) = engine.start().await {
        eprintln!("Start offline ({e}), Outbox läuft trotzdem");
    }
    if let Cmd::Enqueue { token, text, file, voice } = &cli.cmd {
        let text = text.clone().unwrap_or_default();
        match file {
            Some(path) => {
                // The outbox takes ownership of the file: hand it a copy.
                let staging = dir.join("staging");
                std::fs::create_dir_all(&staging)?;
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file")
                    .to_string();
                let copy = staging.join(format!("{}-{name}", new_reference_id()));
                std::fs::copy(path, &copy).with_context(|| format!("{}", path.display()))?;
                if *voice {
                    engine.enqueue_voice_file(token, &copy.to_string_lossy(), &name, None, &new_reference_id())?;
                } else {
                    engine.enqueue_file(
                        token,
                        &copy.to_string_lossy(),
                        &name,
                        mime_for(&name),
                        0,
                        0,
                        &text,
                        None,
                        &new_reference_id(),
                    )?;
                }
            }
            None if !text.is_empty() => {
                engine.enqueue_text(token, &text, None, &new_reference_id());
            }
            None => bail!("--text oder --file angeben"),
        }
    }
    if let Cmd::Voice { token, input, abort_after, draft } = &cli.cmd {
        record_voice(&engine, token, input, *abort_after)?;
        if *draft {
            engine.shutdown();
            return Ok(());
        }
        send_draft(&engine, token)?;
    }
    if let Cmd::VoiceDraft { token, send } = &cli.cmd {
        match engine.voice_draft(token) {
            Some(d) => {
                println!("Entwurf {}: {} ms, {} Bytes, {}", d.id, d.duration_ms, d.size, d.path);
                if *send {
                    send_draft(&engine, token)?;
                } else {
                    engine.shutdown();
                    return Ok(());
                }
            }
            None => {
                println!("kein Entwurf");
                engine.shutdown();
                return Ok(());
            }
        }
    }
    if let Cmd::Fetch { path, size } = &cli.cmd {
        let t = Instant::now();
        let local = engine.fetch_file(path, *size).await?;
        println!("{local} ({:?})", t.elapsed());
        let info = engine.audio_info(&local).await;
        println!("{info:?}");
        engine.shutdown();
        return Ok(());
    }
    loop {
        let idle = engine.wait_outbox_idle().await;
        if idle.waiting == 0 {
            eprintln!(
                "Outbox leer nach {:?}{}",
                t.elapsed(),
                if idle.failed > 0 {
                    format!(", {} fehlgeschlagen", idle.failed)
                } else {
                    String::new()
                }
            );
            break;
        }
        // Everything waits for the network: check again shortly.
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    engine.shutdown();
    Ok(())
}

/// Feeds an audio file through the recorder like a microphone would (48 kHz mono).
fn record_voice(engine: &Arc<Engine>, token: &str, input: &Path, abort_after: Option<u64>) -> Result<()> {
    use std::io::Read;
    const RATE: u32 = 48_000;
    let mut ffmpeg = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(input)
        .args(["-f", "s16le", "-ac", "1", "-ar", &RATE.to_string(), "-"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("ffmpeg")?;
    let mut out = ffmpeg.stdout.take().unwrap();
    let rec = engine.start_voice(token, RATE)?;
    let t = Instant::now();
    let mut block = vec![0u8; (RATE / 10 * 2) as usize];
    let mut status = rec.status();
    loop {
        let mut filled = 0;
        while filled < block.len() {
            match out.read(&mut block[filled..])? {
                0 => break,
                n => filled += n,
            }
        }
        if filled < 2 {
            break;
        }
        let pcm: Vec<i16> = block[..filled].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        status = rec.push(&pcm)?;
        if abort_after.is_some_and(|s| status.duration_ms >= s * 1000) {
            eprintln!("Abbruch simuliert nach {} ms Audio", status.duration_ms);
            std::process::exit(0); // no finish, no drop: like a killed app
        }
        if status.limit_reached {
            eprintln!("Limit erreicht");
            break;
        }
        if filled < block.len() {
            break;
        }
    }
    let _ = ffmpeg.kill();
    let draft = rec.finish()?.context("zu kurz, verworfen")?;
    eprintln!(
        "aufgenommen: {} ms in {:?}, {} Bytes → {}",
        status.duration_ms,
        t.elapsed(),
        draft.size,
        draft.path
    );
    Ok(())
}

fn send_draft(engine: &Arc<Engine>, token: &str) -> Result<()> {
    let name = format!("Sprachnachricht CLI {}", new_reference_id().get(..6).unwrap_or_default());
    let p = engine.send_voice_draft(token, &name, None, &new_reference_id())?;
    eprintln!(
        "in der Outbox: {} ({} Bytes)",
        p.message.attachment.as_ref().map(|a| a.name.as_str()).unwrap_or_default(),
        p.message.attachment.as_ref().map(|a| a.size).unwrap_or_default()
    );
    Ok(())
}

fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("txt") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("wav") => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn print_message(m: &ChatMessage) {
    let time = m.timestamp;
    if m.is_system() {
        println!("[{time}] · {}", m.rendered_text());
    } else {
        let file = m
            .attachment()
            .map(|a| format!("  [{} {} {} B]", m.message_type, a.mimetype, a.size))
            .unwrap_or_default();
        println!("[{time}] {}: {}{file}", m.actor_display_name, m.rendered_text());
    }
}

fn one_line(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    } else {
        s
    }
}

fn load_account(cli: &Cli) -> Result<Account> {
    let path = match &cli.env_file {
        Some(p) => p.clone(),
        None => find_env(&std::env::current_dir()?).context(".env nicht gefunden")?,
    };
    let env = parse_env(&path)?;
    let prefix = match cli.account.as_str() {
        "me" => "NC",
        "a" => "BOT_A",
        "b" => "BOT_B",
        other => bail!("unbekannter Account '{other}' (me | a | b)"),
    };
    let get = |k: &str| {
        env.get(k)
            .cloned()
            .with_context(|| format!("{k} fehlt in {}", path.display()))
    };
    Ok(Account {
        server: get("NC_URL")?,
        user: get(&format!("{prefix}_USER"))?,
        app_password: get(&format!("{prefix}_PASS"))?,
    })
}

fn find_env(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|d| d.join(".env"))
        .find(|p| p.is_file())
}

fn parse_env(path: &Path) -> Result<HashMap<String, String>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        .collect())
}
