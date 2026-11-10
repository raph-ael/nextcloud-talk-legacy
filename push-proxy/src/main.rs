//! Push proxy for the Nextcloud push v2 protocol (`apps/notifications`, docs/push-v2.md).
//!
//! The official proxy only serves Nextcloud's own apps. Clients choose their proxy when
//! they register for push, so independent apps run their own: this one forwards the
//! (end-to-end encrypted) notifications to Firebase Cloud Messaging and Apple Push.
//!
//! Endpoints:
//! - `POST /devices`   register a device: `pushToken`, `deviceIdentifier`,
//!   `deviceIdentifierSignature`, `userPublicKey` (form)
//! - `DELETE /devices` unregister: `deviceIdentifier`, `deviceIdentifierSignature`,
//!   `userPublicKey`
//! - `POST /notifications` from Nextcloud servers: `notifications[]` (JSON strings)
//!
//! Configuration via environment, see README.md.

mod apns;
mod fcm;
mod store;
mod verify;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha512};

use store::{Device, Store};

struct App {
    store: Store,
    fcm: Option<fcm::Fcm>,
    apns: Option<apns::Apns>,
    /// Forward `alert` notifications (chat messages). iOS needs a Notification Service
    /// Extension for them, so they are off for APNs unless enabled.
    apns_alerts: bool,
}

fn form(body: &[u8]) -> Vec<(String, String)> {
    form_urlencoded::parse(body).into_owned().collect()
}

fn field<'a>(f: &'a [(String, String)], name: &str) -> Option<&'a str> {
    f.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn bad(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, msg.to_string()).into_response()
}

async fn register(State(app): State<Arc<App>>, body: Bytes) -> Response {
    let f = form(&body);
    let (Some(token), Some(id), Some(sig), Some(key)) = (
        field(&f, "pushToken"),
        field(&f, "deviceIdentifier"),
        field(&f, "deviceIdentifierSignature"),
        field(&f, "userPublicKey"),
    ) else {
        return bad("missing parameter");
    };
    if token.is_empty() || token.len() > 1024 {
        return bad("invalid push token");
    }
    if !verify::device_signature(key, id, sig) {
        tracing::info!(device = %short(id), "register: invalid signature");
        return (StatusCode::FORBIDDEN, "invalid signature").into_response();
    }
    let hash = hex::encode(Sha512::digest(token.as_bytes()));
    let device = Device {
        id: id.to_string(),
        push_token: token.to_string(),
        push_token_hash: hash,
        user_public_key: key.to_string(),
    };
    match app.store.put(&device) {
        Ok(()) => {
            tracing::info!(device = %short(id), kind = %if token.contains(' ') { "apns" } else { "fcm" }, "registered");
            (StatusCode::OK, "ok").into_response()
        }
        Err(e) => {
            tracing::error!("store: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn unregister(State(app): State<Arc<App>>, body: Bytes) -> Response {
    let f = form(&body);
    let (Some(id), Some(sig), Some(key)) = (
        field(&f, "deviceIdentifier"),
        field(&f, "deviceIdentifierSignature"),
        field(&f, "userPublicKey"),
    ) else {
        return bad("missing parameter");
    };
    if !verify::device_signature(key, id, sig) {
        return (StatusCode::FORBIDDEN, "invalid signature").into_response();
    }
    match app.store.delete(id) {
        Ok(()) => {
            tracing::info!(device = %short(id), "unregistered");
            (StatusCode::OK, "ok").into_response()
        }
        Err(e) => {
            tracing::error!("store: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Notification {
    device_identifier: String,
    push_token_hash: String,
    subject: String,
    signature: String,
    #[serde(default)]
    priority: String,
    #[serde(default)]
    r#type: String,
}

/// What happened to one notification.
pub enum Outcome {
    Sent,
    /// The push service says the token is gone: the server should forget the device.
    Unknown,
    Failed,
}

async fn notifications(State(app): State<Arc<App>>, body: Bytes) -> Response {
    let items: Vec<String> = form(&body)
        .into_iter()
        .filter(|(k, _)| k.starts_with("notifications"))
        .map(|(_, v)| v)
        .collect();
    let mut unknown = Vec::new();
    let mut failed = 0u32;
    for raw in items {
        let Ok(n) = serde_json::from_str::<Notification>(&raw) else {
            failed += 1;
            continue;
        };
        let device = match app.store.get(&n.device_identifier) {
            Ok(Some(d)) => d,
            Ok(None) => {
                unknown.push(n.device_identifier.clone());
                continue;
            }
            Err(e) => {
                tracing::error!("store: {e}");
                failed += 1;
                continue;
            }
        };
        let raw_subject = verify::b64(&n.subject).unwrap_or_default();
        if device.push_token_hash != n.push_token_hash
            || !verify::signature(&device.user_public_key, &raw_subject, &n.signature)
        {
            tracing::info!(device = %short(&n.device_identifier), "notification: token hash or signature mismatch");
            failed += 1;
            continue;
        }
        let outcome = send(&app, &device, &n).await;
        match outcome {
            Outcome::Sent => {}
            Outcome::Unknown => {
                let _ = app.store.delete(&device.id);
                unknown.push(device.id.clone());
            }
            Outcome::Failed => failed += 1,
        }
    }
    axum::Json(json!({"unknown": unknown, "failed": failed})).into_response()
}

async fn send(app: &App, device: &Device, n: &Notification) -> Outcome {
    let high = n.priority == "high";
    let kind = if n.r#type.is_empty() { "alert" } else { n.r#type.as_str() };
    tracing::info!(device = %short(&device.id), kind, high, "forwarding");
    // iOS tokens are "<apns> <pushkit>" (as the Talk iOS app registers them).
    if let Some((apns_token, voip_token)) = device.push_token.split_once(' ') {
        let Some(apns) = &app.apns else { return Outcome::Failed };
        match kind {
            "voip" => apns.send(&device.id, voip_token, apns::Kind::Voip, &n.subject, &n.signature).await,
            "background" => apns.send(&device.id, apns_token, apns::Kind::Background, &n.subject, &n.signature).await,
            _ if app.apns_alerts => apns.send(&device.id, apns_token, apns::Kind::Alert, &n.subject, &n.signature).await,
            // Not forwarded (no Notification Service Extension yet): not a failure.
            _ => Outcome::Sent,
        }
    } else {
        let Some(fcm) = &app.fcm else { return Outcome::Failed };
        fcm.send(&device.push_token, &n.subject, &n.signature, high || kind == "voip").await
    }
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "talk_push_proxy=info".into()),
        )
        .init();
    let listen: SocketAddr = env("PUSH_LISTEN").unwrap_or_else(|| "127.0.0.1:8090".into()).parse()?;
    let store = Store::open(&env("PUSH_DB").unwrap_or_else(|| "push-proxy.sqlite".into()))?;
    let fcm = match env("FCM_SERVICE_ACCOUNT") {
        Some(path) => Some(fcm::Fcm::from_file(&path)?),
        None => {
            tracing::warn!("FCM_SERVICE_ACCOUNT not set: Android pushes disabled");
            None
        }
    };
    let apns = match (env("APNS_KEY"), env("APNS_KEY_ID"), env("APNS_TEAM_ID"), env("APNS_TOPIC")) {
        (Some(key), Some(key_id), Some(team), Some(topic)) => Some(apns::Apns::new(&key, &key_id, &team, &topic, store.clone())?),
        _ => {
            tracing::warn!("APNS_KEY/APNS_KEY_ID/APNS_TEAM_ID/APNS_TOPIC not set: iOS pushes disabled");
            None
        }
    };
    let app = Arc::new(App {
        store,
        fcm,
        apns,
        apns_alerts: env("APNS_FORWARD_ALERTS").is_some_and(|v| v == "1" || v == "true"),
    });
    let router = Router::new()
        .route("/devices", post(register).delete(unregister))
        .route("/notifications", post(notifications))
        .route("/health", get(|| async { "ok" }))
        .with_state(app);
    tracing::info!(%listen, "push proxy listening");
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
