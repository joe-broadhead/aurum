//! Cooperative cancellation for long-running local transcription.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Cheap, cloneable cancel flag shared between host and decode worker.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag {
    inner: Arc<AtomicBool>,
}

impl CancelFlag {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cancellation (idempotent).
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }

    /// Clear the flag (e.g. before a new utterance).
    pub fn reset(&self) {
        self.inner.store(false, Ordering::SeqCst);
    }

    pub fn clone_arc(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner)
    }
}
