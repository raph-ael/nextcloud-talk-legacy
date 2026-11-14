//! Pictures and files: avatars, previews, WebDAV (download with `Range`, simple upload),
//! file shares into conversations, and the Login Flow v2.

use std::io::Cursor;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State as AxState};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use serde_json::{Value, json};

use crate::api::App;
use crate::http::{Params, bad_request, base_url, bytes, not_found, ocs, ocs_status};
use crate::state::mime_for;

type S = AxState<App>;

// --- avatars ---------------------------------------------------------------------------------

pub async fn room_avatar(AxState(app): S, Path(token): Path<String>) -> Response {
    let st = app.st.lock().unwrap();
    match st.room(&token).and_then(|r| r.avatar.clone()) {
        Some(png) => bytes("image/png", png.to_vec()),
        None => default_avatar(),
    }
}

/// `/index.php/avatar/{user}/{size}` and `…/{size}/dark`: the same picture in both themes.
pub async fn user_avatar(AxState(app): S, Path(params): Path<Vec<(String, String)>>) -> Response {
    let user = params.iter().find(|(k, _)| k == "user").map(|(_, v)| v.clone()).unwrap_or_default();
    let st = app.st.lock().unwrap();
    match st.person(&user).and_then(|p| p.avatar.clone()) {
        Some(png) => bytes("image/png", png.to_vec()),
        None => default_avatar(),
    }
}

/// Plain grey square (PNG) for unknown users and pictureless groups.
fn default_avatar() -> Response {
    let img = image::RgbImage::from_pixel(64, 64, image::Rgb([0xB0, 0xBE, 0xC5]));
    let mut out = Vec::new();
    let _ = image::DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png);
    bytes("image/png", out)
}

// --- previews --------------------------------------------------------------------------------

/// `/index.php/core/preview?fileId=&x=&y=&a=1`: JPEG fitting into x×y, never upscaled.
pub async fn preview(AxState(app): S, p: Params) -> Response {
    let Some(id) = p.i64("fileId") else { return not_found() };
    let x = p.i64("x").unwrap_or(512).clamp(16, 4096) as u32;
    let y = p.i64("y").unwrap_or(512).clamp(16, 4096) as u32;
    if let Some(hit) = app.previews.lock().unwrap().get(&(id, x, y)) {
        return bytes("image/jpeg", hit.to_vec());
    }
    let data = {
        let st = app.st.lock().unwrap();
        match st.files.get(&id) {
            Some(f) if f.mime.starts_with("image/") => f.data.clone(),
            _ => return not_found(),
        }
    };
    let jpeg = tokio::task::spawn_blocking(move || -> Option<Vec<u8>> {
        let img = image::load_from_memory(&data).ok()?;
        let img = if img.width() > x || img.height() > y { img.thumbnail(x, y) } else { img };
        let mut out = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88);
        image::DynamicImage::ImageRgb8(img.to_rgb8()).write_with_encoder(enc).ok()?;
        Some(out)
    })
    .await
    .ok()
    .flatten();
    match jpeg {
        Some(j) => {
            app.previews.lock().unwrap().insert((id, x, y), Arc::new(j.clone()));
            bytes("image/jpeg", j)
        }
        None => not_found(),
    }
}

// --- WebDAV ----------------------------------------------------------------------------------

/// `/remote.php/dav/files/{user}/{*path}`: GET (with `Range`), PUT, PROPFIND, MKCOL.
pub async fn dav(
    AxState(app): S,
    method: Method,
    Path((_user, path)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = format!("/{}", path.trim_matches('/'));
    match method.as_str() {
        "GET" | "HEAD" => {
            let Some((data, mime)) = find_file(&app, &path) else { return StatusCode::NOT_FOUND.into_response() };
            ranged(&headers, data, &mime, method == Method::HEAD)
        }
        "PUT" => {
            let mut st = app.st.lock().unwrap();
            let exists = st.uploads.contains_key(&path) || st.files.values().any(|f| format!("/{}", f.path) == path);
            if exists && headers.get(header::IF_NONE_MATCH).is_some_and(|v| v == "*") {
                return StatusCode::PRECONDITION_FAILED.into_response();
            }
            st.uploads.insert(path, Arc::new(body.to_vec()));
            StatusCode::CREATED.into_response()
        }
        "MKCOL" => StatusCode::CREATED.into_response(),
        "PROPFIND" => {
            let size = match find_file(&app, &path) {
                Some((d, _)) => Some(d.len()),
                None if path == "/" || path == "/Talk" => None,
                None => return StatusCode::NOT_FOUND.into_response(),
            };
            let len = size.map(|s| format!("<d:getcontentlength>{s}</d:getcontentlength>")).unwrap_or_default();
            let xml = format!(
                r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response><d:href>/remote.php/dav/files/demo{path}</d:href><d:propstat><d:prop>{len}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#
            );
            (StatusCode::MULTI_STATUS, [(header::CONTENT_TYPE, "application/xml; charset=utf-8")], xml).into_response()
        }
        _ => {
            tracing::warn!("nicht implementiert: {method} WebDAV {path}");
            StatusCode::NOT_IMPLEMENTED.into_response()
        }
    }
}

fn find_file(app: &App, path: &str) -> Option<(Arc<Vec<u8>>, String)> {
    let st = app.st.lock().unwrap();
    if let Some(f) = st.files.values().find(|f| format!("/{}", f.path) == path) {
        return Some((f.data.clone(), f.mime.clone()));
    }
    st.uploads.get(path).map(|d| (d.clone(), mime_for(path).to_string()))
}

fn ranged(headers: &HeaderMap, data: Arc<Vec<u8>>, mime: &str, head: bool) -> Response {
    let len = data.len();
    let start = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split('-').next())
        .and_then(|v| v.parse::<usize>().ok());
    let body = |slice: &[u8]| if head { Vec::new() } else { slice.to_vec() };
    match start {
        Some(s) if s >= len => (
            StatusCode::RANGE_NOT_SATISFIABLE,
            [(header::CONTENT_RANGE, format!("bytes */{len}"))],
        )
            .into_response(),
        Some(s) if s > 0 => (
            StatusCode::PARTIAL_CONTENT,
            [
                (header::CONTENT_TYPE, mime.to_string()),
                (header::CONTENT_RANGE, format!("bytes {s}-{}/{len}", len - 1)),
                (header::ACCEPT_RANGES, "bytes".into()),
            ],
            body(&data[s..]),
        )
            .into_response(),
        _ => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.to_string()), (header::ACCEPT_RANGES, "bytes".into())],
            body(&data),
        )
            .into_response(),
    }
}

// --- shares (files posted into a conversation) ----------------------------------------------

/// `POST files_sharing/api/v1/shares` with `shareType=10`: posts the uploaded file as a
/// message (caption, reply and voice flag from `talkMetaData`).
pub async fn share(AxState(app): S, p: Params) -> Response {
    let token = p.str("shareWith").unwrap_or_default();
    let path = format!("/{}", p.str("path").unwrap_or_default().trim_matches('/'));
    let meta: Value = p.str("talkMetaData").and_then(|m| serde_json::from_str(&m).ok()).unwrap_or(json!({}));
    let mut st = app.st.lock().unwrap();
    if p.i64("shareType") != Some(10) {
        return bad_request("shareType");
    }
    if st.room(&token).is_none() {
        return not_found();
    }
    let existing = st.files.values().find(|f| format!("/{}", f.path) == path).map(|f| f.id);
    let file_id = match existing {
        Some(id) => id,
        None => {
            let Some(data) = st.uploads.get(&path).cloned() else { return not_found() };
            let name = path.rsplit('/').next().unwrap_or("Datei").to_string();
            st.add_file(&name, &path, data)
        }
    };
    let me = st.me.clone();
    let caption = meta["caption"].as_str().unwrap_or("").trim().to_string();
    let mut m = st.new_msg(&me, if caption.is_empty() { "{file}".into() } else { caption });
    if meta["messageType"] == "voice-message" {
        m.kind = "voice-message".into();
    }
    m.params.insert("actor".into(), st.user_param(&me));
    m.params.insert("file".into(), st.file_param(file_id));
    m.file = Some(file_id);
    m.reference_id = p.str("referenceId").unwrap_or_default();
    m.reply_to = meta["replyTo"].as_i64();
    st.push_msg(&token, m);
    st.shares.push((path.clone(), token.clone()));
    ocs(json!({ "id": file_id.to_string(), "share_type": 10, "share_with": token, "path": path }))
}

pub async fn shares(AxState(app): S, p: Params) -> Response {
    let path = format!("/{}", p.str("path").unwrap_or_default().trim_matches('/'));
    let st = app.st.lock().unwrap();
    let list: Vec<Value> = st
        .shares
        .iter()
        .filter(|(sp, _)| *sp == path)
        .map(|(sp, t)| json!({ "share_type": 10, "share_with": t, "path": sp }))
        .collect();
    ocs(Value::Array(list))
}

// --- Login Flow v2 (any user, granted when the page opens) ------------------------------------

pub async fn login_start(AxState(app): S, headers: HeaderMap) -> Response {
    let base = base_url(&headers);
    let token = uuid::Uuid::new_v4().simple().to_string();
    app.st
        .lock()
        .unwrap()
        .login_flows
        .insert(token.clone(), crate::state::LoginFlow { base: base.clone(), granted: false });
    axum::Json(json!({
        "poll": { "token": token, "endpoint": format!("{base}/index.php/login/v2/poll") },
        "login": format!("{base}/index.php/login/v2/flow/{token}"),
    }))
    .into_response()
}

pub async fn login_page(AxState(app): S, Path(token): Path<String>) -> Response {
    let mut st = app.st.lock().unwrap();
    let name = st.name_of(&st.me.clone());
    let Some(flow) = st.login_flows.get_mut(&token) else {
        return (StatusCode::NOT_FOUND, Html("<p>Anmeldung abgelaufen.</p>".to_string())).into_response();
    };
    flow.granted = true;
    Html(format!(
        r#"<!doctype html><html lang="de"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Angemeldet</title><style>body{{font-family:system-ui,sans-serif;background:#eef3fb;color:#1d2433;display:grid;place-items:center;min-height:100vh;margin:0}}
main{{background:#fff;padding:32px 40px;border-radius:16px;box-shadow:0 6px 24px #0001;text-align:center;max-width:320px}}</style></head>
<body><main><h1>Angemeldet ✓</h1><p>Willkommen, {name}. Du kannst jetzt zur App zurückkehren.</p></main></body></html>"#
    ))
    .into_response()
}

pub async fn login_poll(AxState(app): S, p: Params) -> Response {
    let token = p.str("token").unwrap_or_default();
    let mut st = app.st.lock().unwrap();
    match st.login_flows.get(&token) {
        Some(f) if f.granted => {
            let base = f.base.clone();
            st.login_flows.remove(&token);
            axum::Json(json!({
                "server": base,
                "loginName": "demo",
                "appPassword": format!("demo-{}", uuid::Uuid::new_v4().simple()),
            }))
            .into_response()
        }
        _ => ocs_status(StatusCode::NOT_FOUND, json!([])),
    }
}
