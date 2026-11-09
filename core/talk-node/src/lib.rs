//! Node-API facade over `talk-core` for the Electron main process.
//!
//! Data crosses the boundary as plain JS objects (serde → napi), typed on the
//! TypeScript side in `desktop/src/shared/core.ts`. Keep this layer thin.

use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use serde_json::Value;
use talk_core as core;

fn to_napi(e: core::Error) -> Error {
    let code = match e {
        core::Error::Unauthorized => "UNAUTHORIZED",
        core::Error::Network(_) => "NETWORK",
        core::Error::LoginExpired => "LOGIN_EXPIRED",
        _ => "SERVER",
    };
    Error::new(Status::GenericFailure, format!("{code}: {e}"))
}

fn json<T: serde::Serialize>(v: &T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| Error::from_reason(e.to_string()))
}

fn reply_preview(v: Option<Value>) -> Result<Option<core::ReplyPreview>> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v)
            .map(Some)
            .map_err(|e| Error::from_reason(format!("replyTo: {e}"))),
    }
}

#[napi]
pub fn core_version() -> String {
    core::VERSION.to_string()
}

/// Random id to attach to an optimistic message; the echo carries the same id.
#[napi]
pub fn new_reference_id() -> String {
    core::new_reference_id()
}

// --- Login Flow v2 ---------------------------------------------------------------

#[napi(object)]
pub struct LoginResult {
    pub server: String,
    pub login_name: String,
    pub app_password: String,
}

#[napi]
pub struct LoginFlow {
    inner: core::LoginFlow,
    cancel: Arc<tokio::sync::Notify>,
}

/// Starts Login Flow v2; open `loginUrl` in the system browser, then `await wait()`.
#[napi]
pub async fn start_login(server: String) -> Result<LoginFlow> {
    let inner = core::LoginFlow::start(&server, core::Platform::Desktop)
        .await
        .map_err(to_napi)?;
    Ok(LoginFlow {
        inner,
        cancel: Arc::new(tokio::sync::Notify::new()),
    })
}

#[napi]
impl LoginFlow {
    #[napi(getter)]
    pub fn login_url(&self) -> String {
        self.inner.login_url.clone()
    }

    /// Resolves after approval in the browser; rejects with `CANCELLED` after `cancel()`.
    #[napi]
    pub async fn wait(&self) -> Result<LoginResult> {
        let c = tokio::select! {
            r = self.inner.wait() => r.map_err(to_napi)?,
            _ = self.cancel.notified() => {
                return Err(Error::new(Status::Cancelled, "CANCELLED: login cancelled"));
            }
        };
        Ok(LoginResult {
            server: c.server,
            login_name: c.login_name,
            app_password: c.app_password,
        })
    }

    #[napi]
    pub fn cancel(&self) {
        self.cancel.notify_one();
    }
}

// --- Session ---------------------------------------------------------------------

type EventCallback = ThreadsafeFunction<Value, Unknown<'static>, Value, Status, false>;

struct JsSink(EventCallback);

impl core::EventSink for JsSink {
    fn emit(&self, event: core::Event) {
        match serde_json::to_value(&event) {
            Ok(v) => {
                self.0.call(v, ThreadsafeFunctionCallMode::NonBlocking);
            }
            Err(e) => tracing::error!("event serialization failed: {e}"),
        }
    }
}

#[napi(object)]
pub struct Avatar {
    pub data: Buffer,
    pub content_type: String,
}

#[napi]
pub struct TalkSession {
    engine: Arc<core::Engine>,
}

#[napi]
impl TalkSession {
    /// `onEvent` receives every core event (`{ type: "roomsChanged", ... }`).
    #[napi(constructor)]
    pub fn new(
        server: String,
        user: String,
        app_password: String,
        #[napi(ts_arg_type = "(event: any) => void")] on_event: EventCallback,
        data_dir: Option<String>,
    ) -> Result<Self> {
        let account = core::Account {
            server,
            user,
            app_password,
        };
        let client = core::TalkClient::new(account, core::Platform::Desktop).map_err(to_napi)?;
        let engine = core::Engine::new(client, Arc::new(JsSink(on_event)), data_dir.map(Into::into));
        Ok(Self { engine })
    }

    /// Resolves the user, loads conversations, starts background sync.
    #[napi]
    pub async fn start(&self) -> Result<()> {
        self.engine.start().await.map_err(to_napi)
    }

    #[napi]
    pub fn shutdown(&self) {
        self.engine.shutdown();
    }

    #[napi(getter)]
    pub fn user_id(&self) -> String {
        self.engine.user_id()
    }

    #[napi(ts_return_type = "any[]")]
    pub fn rooms(&self) -> Result<Value> {
        json(&self.engine.rooms())
    }

    #[napi]
    pub async fn refresh_rooms(&self) -> Result<()> {
        self.engine.refresh_rooms(true).await.map_err(to_napi)
    }

    /// Newest cached messages (empty without cache), for the first frame of a chat.
    #[napi(ts_return_type = "any")]
    pub fn cached_chat(&self, token: String) -> Result<Value> {
        json(&self.engine.cached_chat(&token))
    }

    #[napi(ts_return_type = "Promise<any>")]
    pub async fn open_chat(&self, token: String) -> Result<Value> {
        json(&self.engine.open_chat(&token).await.map_err(to_napi)?)
    }

    #[napi]
    pub fn close_chat(&self, token: String) {
        self.engine.close_chat(&token);
    }

    #[napi(ts_return_type = "Promise<any>")]
    pub async fn load_older(&self, token: String, before_id: i64) -> Result<Value> {
        json(&self.engine.load_older(&token, before_id).await.map_err(to_napi)?)
    }

    #[napi(ts_return_type = "Promise<any>")]
    pub async fn send(
        &self,
        token: String,
        text: String,
        reference_id: String,
        reply_to: Option<i64>,
    ) -> Result<Value> {
        let msg = self
            .engine
            .send(&token, &text, &reference_id, reply_to)
            .await
            .map_err(to_napi)?;
        json(&msg)
    }

    #[napi]
    pub async fn mark_read(&self, token: String, message_id: i64) -> Result<()> {
        self.engine
            .mark_read(&token, message_id)
            .await
            .map_err(to_napi)
    }

    /// Revokes the app password on the server and stops all sync.
    #[napi]
    pub async fn logout(&self) -> Result<()> {
        self.engine.logout().await.map_err(to_napi)
    }

    // --- outbox: reliable sending ---------------------------------------------------

    /// Queues a text message; delivered in the background (retried, survives restarts).
    /// `replyTo` is a `ReplyPreview` (`{ id, author, text, isOwn, attachment }`).
    #[napi(ts_return_type = "any")]
    pub fn enqueue_text(
        &self,
        token: String,
        text: String,
        #[napi(ts_arg_type = "any")] reply_to: Option<Value>,
        reference_id: String,
    ) -> Result<Value> {
        let reply = reply_preview(reply_to)?;
        json(&self.engine.enqueue_text(&token, &text, reply, &reference_id))
    }

    /// Queues a file. `localPath` is moved into the outbox; don't use it afterwards.
    #[allow(clippy::too_many_arguments)]
    #[napi(ts_return_type = "any")]
    pub fn enqueue_file(
        &self,
        token: String,
        local_path: String,
        name: String,
        mime: String,
        width: i64,
        height: i64,
        caption: String,
        #[napi(ts_arg_type = "any")] reply_to: Option<Value>,
        reference_id: String,
    ) -> Result<Value> {
        let reply = reply_preview(reply_to)?;
        let p = self
            .engine
            .enqueue_file(
                &token,
                &local_path,
                &name,
                &mime,
                width,
                height,
                &caption,
                reply,
                &reference_id,
            )
            .map_err(to_napi)?;
        json(&p)
    }

    /// Pending and failed messages of a conversation, in send order.
    #[napi(ts_return_type = "any[]")]
    pub fn pending(&self, token: String) -> Result<Value> {
        json(&self.engine.pending(&token))
    }

    #[napi]
    pub fn retry_send(&self, reference_id: String) {
        self.engine.retry_send(&reference_id);
    }

    #[napi]
    pub fn discard(&self, reference_id: String) {
        self.engine.discard(&reference_id);
    }

    /// Call when connectivity changes: waiting items are retried at once.
    #[napi]
    pub fn network_changed(&self) {
        self.engine.network_changed();
    }

    #[napi]
    pub fn outbox_unsent(&self) -> u32 {
        self.engine.outbox_unsent()
    }

    /// Resolves once nothing is sending anymore (`{ waiting, failed }`).
    #[napi(ts_return_type = "Promise<any>")]
    pub async fn wait_outbox_idle(&self) -> Result<Value> {
        json(&self.engine.wait_outbox_idle().await)
    }

    /// Preview image (JPEG/PNG bytes) of a shared file, fitting into `width`×`height`.
    #[napi]
    pub async fn file_preview(&self, file_id: String, width: u32, height: u32) -> Result<Buffer> {
        let data = self
            .engine
            .file_preview(&file_id, width, height)
            .await
            .map_err(to_napi)?;
        Ok(data.into())
    }

    #[napi]
    pub async fn avatar(&self, token: String, dark: bool) -> Result<Avatar> {
        let (data, content_type) = self.engine.avatar(&token, dark).await.map_err(to_napi)?;
        Ok(Avatar {
            data: data.into(),
            content_type,
        })
    }
}
