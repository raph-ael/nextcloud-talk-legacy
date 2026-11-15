//! One call per process. Several accounts (engines) run side by side, but there is one
//! microphone and one call screen: whoever rings or talks holds the slot, and every other
//! engine is "busy" for that time (docs/accounts.md, docs/calls.md).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::actor::Shared;
use crate::engine::{Event, EventSink};

/// Who holds the slot: an engine and the call it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Holder {
    pub owner: u64,
    /// [`crate::accounts::server_key`] of the account.
    pub server: String,
    pub token: String,
}

#[derive(Default)]
pub(crate) struct CallSlot {
    holder: Mutex<Option<Holder>>,
}

impl CallSlot {
    /// The slot every engine of this process shares.
    pub(crate) fn global() -> Arc<CallSlot> {
        static GLOBAL: OnceLock<Arc<CallSlot>> = OnceLock::new();
        GLOBAL.get_or_init(Default::default).clone()
    }

    /// A fresh id for a [`super::CallManager`].
    pub(crate) fn next_owner() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn holder(&self) -> Option<Holder> {
        self.holder.lock().unwrap().clone()
    }

    /// Takes the slot, or returns `None` when a call exists already (in any engine).
    pub(crate) fn acquire(self: &Arc<Self>, holder: Holder) -> Option<SlotGuard> {
        let mut slot = self.holder.lock().unwrap();
        if slot.is_some() {
            return None;
        }
        let owner = holder.owner;
        *slot = Some(holder);
        Some(SlotGuard { slot: self.clone(), owner })
    }
}

/// Frees the slot when dropped.
pub(crate) struct SlotGuard {
    slot: Arc<CallSlot>,
    owner: u64,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        let mut slot = self.slot.holder.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|h| h.owner == self.owner) {
            *slot = None;
        }
    }
}

/// The published side of a running call, owned by its task. Ending it clears the engine's
/// call state, tells the platform (`CallChanged { call: None }`) and only then frees the
/// slot, so the next call's first event never overtakes this one's last. Dropping it
/// without [`Live::end`] (the task panicked or its runtime went away) does the same, so
/// a broken call cannot block every account.
pub(crate) struct Live {
    shared: Arc<Shared>,
    sink: Arc<dyn EventSink>,
    ended: bool,
    _slot: SlotGuard,
}

impl Live {
    pub(crate) fn new(shared: Arc<Shared>, sink: Arc<dyn EventSink>, slot: SlotGuard) -> Self {
        Self { shared, sink, ended: false, _slot: slot }
    }

    pub(crate) fn end(&mut self) {
        if self.ended {
            return;
        }
        self.ended = true;
        *self.shared.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.shared.view.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.sink.emit(Event::CallChanged { call: None });
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.end();
    }
}
