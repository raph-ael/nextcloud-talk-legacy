//! Websocket client on top of reqwest's HTTP/1.1 upgrade, so websockets use exactly the
//! TLS configuration of the API client (Android system roots, platform verifier).

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::header;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::Message;

use crate::error::{Error, Result};

pub type WebSocket = WebSocketStream<reqwest::Upgraded>;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

fn ws_error(e: impl std::fmt::Display) -> Error {
    Error::Call {
        code: "signaling".into(),
        message: e.to_string(),
    }
}

/// `wss://…` or `https://…` (both accepted).
pub(crate) async fn connect(http: &reqwest::Client, url: &str) -> Result<WebSocket> {
    let url = if let Some(rest) = url.strip_prefix("wss://") {
        format!("https://{rest}")
    } else if let Some(rest) = url.strip_prefix("ws://") {
        format!("http://{rest}")
    } else {
        url.to_string()
    };
    let key = generate_key();
    let fut = async {
        let resp = http
            .get(&url)
            .header(header::CONNECTION, "Upgrade")
            .header(header::UPGRADE, "websocket")
            .header(header::SEC_WEBSOCKET_VERSION, "13")
            .header(header::SEC_WEBSOCKET_KEY, &key)
            .send()
            .await?;
        if resp.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
            return Err(Error::Http {
                status: resp.status().as_u16(),
                message: "websocket upgrade refused".into(),
            });
        }
        let accept = resp
            .headers()
            .get(header::SEC_WEBSOCKET_ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        if accept != derive_accept_key(key.as_bytes()) {
            return Err(ws_error("websocket accept key mismatch"));
        }
        let io = resp.upgrade().await?;
        let mut config = WebSocketConfig::default();
        config.max_message_size = Some(4 << 20);
        Ok(WebSocketStream::from_raw_socket(io, Role::Client, Some(config)).await)
    };
    tokio::time::timeout(HANDSHAKE_TIMEOUT, fut)
        .await
        .map_err(|_| ws_error("websocket handshake timed out"))?
}

/// Sends a text frame.
pub(crate) async fn send_text(ws: &mut WebSocket, text: String) -> Result<()> {
    ws.send(Message::text(text)).await.map_err(ws_error)
}

pub(crate) async fn send_ping(ws: &mut WebSocket) -> Result<()> {
    ws.send(Message::Ping(Vec::new().into())).await.map_err(ws_error)
}

/// What a websocket read yields for the call logic.
pub(crate) enum Incoming {
    Text(String),
    /// Any frame (also pong) proves the connection is alive.
    Alive,
    Closed(String),
}

pub(crate) async fn next(ws: &mut WebSocket) -> Incoming {
    match ws.next().await {
        Some(Ok(Message::Text(t))) => Incoming::Text(t.to_string()),
        Some(Ok(Message::Close(c))) => Incoming::Closed(c.map(|c| c.reason.to_string()).unwrap_or_default()),
        Some(Ok(_)) => Incoming::Alive,
        Some(Err(e)) => Incoming::Closed(e.to_string()),
        None => Incoming::Closed("eof".into()),
    }
}
