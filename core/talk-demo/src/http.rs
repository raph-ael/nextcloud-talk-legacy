//! HTTP helpers: the OCS envelope and a parameter extractor that reads query, form and
//! JSON bodies alike (the clients send forms for most calls, JSON for categories).

use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

/// `{"ocs":{"meta":{…},"data":…}}` with HTTP 200.
pub fn ocs(data: Value) -> Response {
    ocs_status(StatusCode::OK, data)
}

pub fn ocs_status(status: StatusCode, data: Value) -> Response {
    let ok = status.is_success();
    let body = json!({
        "ocs": {
            "meta": {
                "status": if ok { "ok" } else { "failure" },
                "statuscode": status.as_u16(),
                "message": if ok { "OK" } else { status.canonical_reason().unwrap_or("") },
            },
            "data": data,
        }
    });
    (status, axum::Json(body)).into_response()
}

pub fn ocs_with_headers(status: StatusCode, data: Value, headers: &[(&'static str, String)]) -> Response {
    let mut resp = ocs_status(status, data);
    add_headers(resp.headers_mut(), headers);
    resp
}

pub fn add_headers(map: &mut HeaderMap, headers: &[(&'static str, String)]) {
    for (k, v) in headers {
        if let Ok(v) = HeaderValue::from_str(v) {
            map.insert(HeaderName::from_static(k), v);
        }
    }
}

pub fn not_found() -> Response {
    ocs_status(StatusCode::NOT_FOUND, json!([]))
}

pub fn bad_request(error: &str) -> Response {
    ocs_status(StatusCode::BAD_REQUEST, json!({ "error": error }))
}

pub fn bytes(content_type: &str, data: Vec<u8>) -> Response {
    (
        [(header::CONTENT_TYPE, content_type.to_string()), (header::CACHE_CONTROL, "max-age=3600".into())],
        data,
    )
        .into_response()
}

/// All request parameters in order: query string first, then the body (form or JSON).
/// Repeated keys (`participants[users][]`) stay repeated.
#[derive(Debug, Default)]
pub struct Params(pub Vec<(String, Value)>);

impl Params {
    pub fn str(&self, key: &str) -> Option<String> {
        self.0.iter().rev().find(|(k, _)| k == key).map(|(_, v)| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }

    pub fn i64(&self, key: &str) -> Option<i64> {
        self.str(key)?.trim().parse().ok()
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.0.iter().rev().find(|(k, _)| k == key).map(|(_, v)| match v {
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_i64() != Some(0),
            Value::String(s) => matches!(s.as_str(), "1" | "true" | "yes"),
            _ => false,
        })
    }

    /// Values of a list parameter: a JSON array, or repeated `key[]` / `key` form fields.
    pub fn list(&self, key: &str) -> Vec<String> {
        let bracket = format!("{key}[]");
        let mut out = Vec::new();
        for (k, v) in &self.0 {
            if k != key && *k != bracket {
                continue;
            }
            match v {
                Value::Array(a) => out.extend(a.iter().map(|x| match x {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })),
                Value::String(s) => out.push(s.clone()),
                Value::Number(n) => out.push(n.to_string()),
                _ => {}
            }
        }
        out
    }
}

impl<S: Send + Sync> FromRequest<S> for Params {
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let mut out = Vec::new();
        if let Some(q) = req.uri().query() {
            push_form(&mut out, q.as_bytes());
        }
        let json_body = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("json"));
        let body = Bytes::from_request(req, state)
            .await
            .map_err(|_| bad_request("body"))?;
        if !body.is_empty() {
            if json_body {
                if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&body) {
                    out.extend(map);
                }
            } else {
                push_form(&mut out, &body);
            }
        }
        Ok(Params(out))
    }
}

fn push_form(out: &mut Vec<(String, Value)>, raw: &[u8]) {
    if let Ok(pairs) = serde_urlencoded::from_bytes::<Vec<(String, String)>>(raw) {
        out.extend(pairs.into_iter().map(|(k, v)| (k, Value::String(v))));
    }
}

/// `http://<Host>` of the request, for absolute URLs in the login flow.
pub fn base_url(headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1:8099");
    format!("http://{host}")
}
