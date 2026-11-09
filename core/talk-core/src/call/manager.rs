//! Owner of the single active call: state machine, signaling and media commands.

use std::sync::{Arc, Mutex, RwLock};

use super::{CallView, MediaEngine, MediaReport};
use crate::error::{Error, Result};

#[derive(Default)]
pub(crate) struct CallManager {
    media: RwLock<Option<Arc<dyn MediaEngine>>>,
    current: Mutex<Option<CallView>>,
}

fn not_yet() -> Error {
    Error::Call {
        code: "not-implemented".into(),
        message: "calls are not implemented yet".into(),
    }
}

impl CallManager {
    pub(crate) fn set_media_engine(&self, media: Arc<dyn MediaEngine>) {
        *self.media.write().unwrap() = Some(media);
    }

    pub(crate) fn current(&self) -> Option<CallView> {
        self.current.lock().unwrap().clone()
    }

    pub(crate) async fn start(&self, _token: &str, _silent: bool) -> Result<()> {
        Err(not_yet())
    }

    pub(crate) async fn accept(&self, _token: &str) -> Result<()> {
        Err(not_yet())
    }

    pub(crate) fn decline(&self, _token: &str) {}

    pub(crate) fn hang_up(&self) {}

    pub(crate) fn set_muted(&self, _muted: bool) {}

    pub(crate) fn report(&self, _report: MediaReport) {}
}
