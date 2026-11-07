//! `talk-cli` – exercise the shared core against a real server.
//!
//! Credentials come from a `.env` file (searched upwards from the current directory):
//! `NC_URL` plus `NC_USER/NC_PASS` (account `me`), `BOT_A_*` (`a`), `BOT_B_*` (`b`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use talk_core::{Account, ChatMessage, LoginFlow, Platform, TalkClient, new_reference_id};

#[derive(Parser)]
#[command(version, about = "Talk core test CLI")]
struct Cli {
    /// Account from .env: me | a | b
    #[arg(short, long, default_value = "me", global = true)]
    account: String,
    /// Path to .env (default: search upwards from the current directory)
    #[arg(long, global = true)]
    env_file: Option<PathBuf>,
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

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

    match cli.cmd {
        Cmd::Login { .. } => unreachable!(),
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

fn print_message(m: &ChatMessage) {
    let time = m.timestamp;
    if m.is_system() {
        println!("[{time}] · {}", m.rendered_text());
    } else {
        println!("[{time}] {}: {}", m.actor_display_name, m.rendered_text());
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
