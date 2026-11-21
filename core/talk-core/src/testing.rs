//! A small HTTP server for tests of what the engine asks a server for.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::client::{Account, Platform, TalkClient};
use crate::engine::{Engine, Event, EventSink};

pub(crate) struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
}

impl Reply {
    /// An OCS answer with `data`.
    pub fn ocs(data: serde_json::Value) -> Self {
        let body = serde_json::json!({ "ocs": { "meta": { "status": "ok", "statuscode": 200 }, "data": data } });
        Self { status: 200, headers: Vec::new(), body: body.to_string() }
    }

    pub fn status(status: u16) -> Self {
        Self { status, headers: Vec::new(), body: String::new() }
    }

    pub fn header(mut self, name: &'static str, value: impl ToString) -> Self {
        self.headers.push((name, value.to_string()));
        self
    }
}

pub(crate) struct Server {
    pub url: String,
    /// Path and query of every request, in the order they came in.
    pub seen: Arc<Mutex<Vec<String>>>,
}

impl Server {
    pub fn seen(&self, part: &str) -> Vec<String> {
        self.seen.lock().unwrap().iter().filter(|p| p.contains(part)).cloned().collect()
    }
}

/// Answers every request with what `handler` makes of its path and query. One thread per
/// connection, so a handler may take its time while others are served.
pub(crate) fn serve(handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (handler, log) = (handler.clone(), log.clone());
            std::thread::spawn(move || {
                let mut lines = BufReader::new(stream.try_clone().unwrap()).lines();
                let Some(Ok(first)) = lines.next() else { return };
                // Up to the empty line: the requests of these tests have no body.
                for line in lines {
                    if line.map_or(true, |l| l.is_empty()) {
                        break;
                    }
                }
                let path = first.split(' ').nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push(path.clone());
                let reply = handler(&path);
                let mut out = format!("HTTP/1.1 {} X\r\nContent-Type: application/json\r\nConnection: close\r\n", reply.status);
                for (name, value) in &reply.headers {
                    out.push_str(&format!("{name}: {value}\r\n"));
                }
                if reply.status != 304 {
                    out.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
                }
                out.push_str("\r\n");
                if reply.status != 304 {
                    out.push_str(&reply.body);
                }
                let _ = stream.write_all(out.as_bytes());
            });
        }
    });
    Server { url, seen }
}

/// The value of a query parameter.
pub(crate) fn param(path: &str, name: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| pair.strip_prefix(name)?.strip_prefix('=').map(str::to_string))
}

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _: Event) {}
}

/// A fresh data directory for a test.
pub(crate) fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("talk-core-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// An engine of the user "me" on `server`.
pub(crate) fn engine(server: &Server, data_dir: Option<PathBuf>) -> Arc<Engine> {
    let account = Account { server: server.url.clone(), user: "me".into(), app_password: "x".into() };
    Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), Arc::new(Quiet), data_dir)
}
