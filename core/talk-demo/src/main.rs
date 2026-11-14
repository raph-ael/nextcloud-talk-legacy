//! Demo server for website screenshots: speaks enough of the Nextcloud/Talk API for the
//! real clients (Android, iOS, desktop, talk-cli) and serves invented content from
//! `fixtures/`. Plain HTTP, any user name and password; everyone is the fixture's `me`.
//!
//!     cargo run -p talk-demo -- --listen 0.0.0.0:8099
//!
//! Calls and signaling are out of scope. Unknown endpoints are logged as
//! "nicht implementiert" and answered with an empty OCS success.

#![recursion_limit = "256"]

mod api;
mod files;
mod http;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use clap::Parser;

use crate::api::{App, AppState};

#[derive(Parser)]
#[command(version, about = "Talk-Demo-Server mit erfundenen Inhalten (für Screenshots)")]
struct Args {
    /// Address to listen on, e.g. 0.0.0.0:8099 for devices in the LAN.
    #[arg(long, default_value = "127.0.0.1:8099")]
    listen: SocketAddr,
    /// Directory with people.json, conversations.json, avatars/ and media/.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures"))]
    fixtures: PathBuf,
}

const SPREED: &str = "/ocs/v2.php/apps/spreed/api";

pub fn router(app: App) -> Router {
    let sp = |p: &str| format!("{SPREED}{p}");
    Router::new()
        .route("/", get(api::root))
        .route("/status.php", get(api::status_php))
        .route("/index.php/login/v2", post(files::login_start))
        .route("/index.php/login/v2/flow/{token}", get(files::login_page))
        .route("/index.php/login/v2/poll", post(files::login_poll))
        .route("/ocs/v2.php/cloud/capabilities", get(api::capabilities))
        .route("/ocs/v2.php/cloud/user", get(api::cloud_user))
        .route("/ocs/v2.php/core/getapppassword", get(api::get_app_password))
        .route("/ocs/v2.php/core/apppassword", axum::routing::delete(api::delete_app_password))
        .route("/ocs/v2.php/core/autocomplete/get", get(api::autocomplete))
        .route("/ocs/v2.php/apps/notifications/api/v2/notifications", get(api::notifications))
        // conversations
        .route(&sp("/v4/room"), get(api::rooms).post(api::create_room))
        .route(&sp("/v4/room/{token}"), get(api::room).put(api::rename_room).delete(api::delete_room))
        .route(&sp("/v4/room/{token}/description"), put(api::set_description))
        .route(&sp("/v4/room/{token}/favorite"), post(api::favorite_on).delete(api::favorite_off))
        .route(&sp("/v4/room/{token}/archive"), post(api::archive_on).delete(api::archive_off))
        .route(&sp("/v4/room/{token}/notify"), post(api::notify_level))
        .route(&sp("/v4/room/{token}/tags"), post(api::room_tags))
        .route(&sp("/v4/room/{token}/participants"), get(api::participants).post(api::add_participant))
        .route(&sp("/v4/room/{token}/participants/active"), post(api::join).delete(api::leave))
        .route(&sp("/v4/room/{token}/participants/self"), axum::routing::delete(api::leave_conversation))
        .route(&sp("/v4/room/{token}/attendees"), axum::routing::delete(api::remove_attendee))
        .route(&sp("/v4/room/{token}/moderators"), post(api::promote).delete(api::demote))
        .route(&sp("/v1/room/{token}/avatar"), get(files::room_avatar))
        .route(&sp("/v1/room/{token}/avatar/dark"), get(files::room_avatar))
        // chat
        .route(&sp("/v1/chat/{token}"), get(api::chat_get).post(api::chat_post))
        .route(&sp("/v1/chat/{token}/read"), post(api::read_marker).delete(api::unread_marker))
        .route(
            &sp("/v1/reaction/{token}/{id}"),
            get(api::reactions_get).post(api::reaction_add).delete(api::reaction_remove),
        )
        // categories
        .route(&sp("/v4/tags"), get(api::tags).post(api::create_tag))
        .route(&sp("/v4/tags/reorder"), put(api::reorder_tags))
        .route(&sp("/v4/tags/{id}"), put(api::rename_tag).delete(api::delete_tag))
        .route(&sp("/v4/tags/{id}/collapsed"), put(api::collapse_tag))
        // files and pictures
        .route("/index.php/avatar/{user}/{size}", get(files::user_avatar))
        .route("/index.php/avatar/{user}/{size}/dark", get(files::user_avatar))
        .route("/index.php/core/preview", get(files::preview))
        .route("/index.php/core/preview.png", get(files::preview))
        .route("/remote.php/dav/files/{user}/{*path}", any(files::dav))
        .route("/ocs/v2.php/apps/files_sharing/api/v1/shares", get(files::shares).post(files::share))
        .fallback(api::unknown)
        .method_not_allowed_fallback(api::unknown)
        .layer(middleware::from_fn(auth))
        .layer(middleware::from_fn(log))
        .with_state(app)
}

/// Any Basic auth is fine; without one, 401 like Nextcloud (the clients then sign out).
async fn auth(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let public = path == "/" || path == "/status.php" || path.starts_with("/index.php/login/v2");
    let has_auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Basic "));
    if !public && !has_auth {
        return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic realm=\"Demo\"")]).into_response();
    }
    next.run(req).await
}

async fn log(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let start = Instant::now();
    let resp = next.run(req).await;
    tracing::info!("{method} {uri} → {} ({} ms)", resp.status().as_u16(), start.elapsed().as_millis());
    resp
}

pub fn app_from(dir: &std::path::Path) -> Result<App, String> {
    let st = state::State::load(dir)?;
    Ok(Arc::new(AppState { st: Mutex::new(st), previews: Mutex::default() }))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let app = match app_from(&args.fixtures) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Fixtures in {}: {e}", args.fixtures.display());
            std::process::exit(1);
        }
    };
    {
        let st = app.st.lock().unwrap();
        tracing::info!(
            "{} Unterhaltungen, angemeldet ist immer {} ({})",
            st.rooms.len(),
            st.name_of(&st.me),
            st.me
        );
    }
    let listener = match tokio::net::TcpListener::bind(args.listen).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}: {e}", args.listen);
            std::process::exit(1);
        }
    };
    tracing::info!("Demo-Server läuft auf http://{} (beliebiger Benutzer/Passwort)", args.listen);
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("server");
}

#[cfg(test)]
mod tests;
