//! Per-operation control context (JOE-1595).

use crate::cancel::CancelFlag;
use crate::error::{ProviderError, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Progress event for hosts/telemetry (best-effort).
#[derive(Debug, Clone)]
pub struct OpProgress {
    pub stage: &'static str,
    pub detail: String,
}

/// Optional sink for progress events.
pub type ProgressSink = Arc<dyn Fn(OpProgress) + Send + Sync>;

/// Immutable-ish control context for one operation.
///
/// Fresh cancel token per operation — never reset a shared engine flag across ops.
///
/// Deadline uses a single absolute monotonic [`Instant`]; nested APIs should call
/// [`Self::remaining`] rather than stacking independent timeouts.
#[derive(Clone)]
pub struct OpContext {
    pub request_id: u64,
    pub cancel: CancelFlag,
    /// Absolute deadline (monotonic). None = no deadline.
    deadline: Option<Instant>,
    pub progress: Option<ProgressSink>,
}

impl std::fmt::Debug for OpContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpContext")
            .field("request_id", &self.request_id)
            .field("cancelled", &self.cancel.is_cancelled())
            .field("has_deadline", &self.deadline.is_some())
            .finish()
    }
}

impl Default for OpContext {
    fn default() -> Self {
        Self::new()
    }
}

impl OpContext {
    pub fn new() -> Self {
        Self {
            request_id: NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
            cancel: CancelFlag::new(),
            deadline: None,
            progress: None,
        }
    }

    /// Create from an optional host cancel flag (cloned ownership of the same token).
    pub fn with_cancel(cancel: CancelFlag) -> Self {
        Self {
            request_id: NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
            cancel,
            deadline: None,
            progress: None,
        }
    }

    /// Build from an optional cancel flag, creating a fresh token when absent.
    pub fn from_optional_cancel(cancel: Option<CancelFlag>) -> Self {
        match cancel {
            Some(c) => Self::with_cancel(c),
            None => Self::new(),
        }
    }

    pub fn with_deadline_from_now(mut self, timeout: Duration) -> Self {
        if !timeout.is_zero() {
            self.deadline = Some(Instant::now() + timeout);
        }
        self
    }

    pub fn with_absolute_deadline(mut self, at: Instant) -> Self {
        self.deadline = Some(at);
        self
    }

    pub fn with_progress(mut self, sink: ProgressSink) -> Self {
        self.progress = Some(sink);
        self
    }

    /// Override the auto-assigned request id (JOE-2221 host-stable ids).
    pub fn with_request_id(mut self, id: u64) -> Self {
        self.request_id = id;
        self
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
    }

    /// Check cancel then deadline. Prefer cancel when both apply.
    pub fn check(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(ProviderError::Cancelled.into());
        }
        if let Some(d) = self.deadline {
            if Instant::now() >= d {
                return Err(ProviderError::DeadlineExceeded.into());
            }
        }
        Ok(())
    }

    pub fn emit(&self, stage: &'static str, detail: impl Into<String>) {
        if let Some(sink) = &self.progress {
            sink(OpProgress {
                stage,
                detail: detail.into(),
            });
        }
    }

    /// Tokio-compatible timeout duration for the remaining budget (or fallback).
    pub fn timeout_or(&self, fallback: Duration) -> Duration {
        self.remaining().unwrap_or(fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_beats_deadline() {
        let ctx = OpContext::new().with_deadline_from_now(Duration::from_secs(60));
        ctx.cancel.cancel();
        let err = ctx.check().unwrap_err();
        assert!(matches!(
            err,
            crate::error::TranscriptionError::Provider(ProviderError::Cancelled)
        ));
    }

    #[test]
    fn deadline_fires() {
        let ctx = OpContext::new().with_deadline_from_now(Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(5));
        let err = ctx.check().unwrap_err();
        assert!(matches!(
            err,
            crate::error::TranscriptionError::Provider(ProviderError::DeadlineExceeded)
        ));
    }

    #[test]
    fn unique_request_ids() {
        let a = OpContext::new();
        let b = OpContext::new();
        assert_ne!(a.request_id, b.request_id);
    }
}
