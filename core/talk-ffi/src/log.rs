//! Core logs to the platform's log (Android: logcat via `android.util.Log`, iOS: `os.Logger`).

use std::fmt::Write as _;
use std::sync::Arc;

use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

#[derive(uniffi::Enum, Clone, Copy)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Receives formatted core log lines. Called from any thread; must not block.
#[uniffi::export(with_foreign)]
pub trait LogSink: Send + Sync {
    fn log(&self, level: LogLevel, target: String, message: String);
}

struct Forward(Arc<dyn LogSink>);

#[derive(Default)]
struct Line {
    message: String,
    fields: String,
}

impl Visit for Line {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={value}", field.name());
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for Forward {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = match *meta.level() {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::TRACE => LogLevel::Trace,
        };
        let mut line = Line::default();
        event.record(&mut line);
        self.0.log(level, meta.target().to_string(), format!("{}{}", line.message, line.fields));
    }
}

/// Routes core logs to `sink`. `filter` like `RUST_LOG`, e.g. "talk_core=info" (default)
/// or "talk_core=debug,talk_core::call=trace". Only the first call takes effect.
#[uniffi::export]
pub fn init_logging(sink: Arc<dyn LogSink>, filter: Option<String>) {
    let filter = EnvFilter::new(filter.unwrap_or_else(|| "talk_core=info".into()));
    let _ = tracing_subscriber::registry()
        .with(Forward(sink).with_filter(filter))
        .try_init();
}
