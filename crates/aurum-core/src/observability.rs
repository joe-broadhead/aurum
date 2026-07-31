//! Structured operation metrics and privacy-safe diagnostics (JOE-1627).
//!
//! Counters are process-local, lock-free, and never store payload text/PCM.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Schema version for metrics / diagnostic JSON.
pub const METRICS_SCHEMA_VERSION: u32 = 1;

/// Process-wide metrics sink (also attachable to an engine via [`Arc`]).
#[derive(Debug, Default)]
pub struct Metrics {
    pub ops_started: AtomicU64,
    pub ops_completed: AtomicU64,
    pub ops_failed: AtomicU64,
    pub ops_cancelled: AtomicU64,
    pub ops_deadline: AtomicU64,
    pub queue_wait_ms_total: AtomicU64,
    pub inference_ms_total: AtomicU64,
    pub model_loads: AtomicU64,
    pub remote_errors: AtomicU64,
    pub busy_rejections: AtomicU64,
    pub overload_rejections: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process-global shared metrics sink (same instance as [`process_metrics`]).
    pub fn shared() -> Arc<Self> {
        process_metrics_arc()
    }

    pub fn record_start(&self) {
        self.ops_started.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_complete(&self, inference: Duration) {
        self.ops_completed.fetch_add(1, Ordering::Relaxed);
        self.inference_ms_total
            .fetch_add(inference.as_millis() as u64, Ordering::Relaxed);
    }

    pub fn record_failed(&self) {
        self.ops_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_cancelled(&self) {
        self.ops_cancelled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_deadline(&self) {
        self.ops_deadline.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_model_load(&self) {
        self.model_loads.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_busy(&self) {
        self.busy_rejections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_overload(&self) {
        self.overload_rejections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_queue_wait(&self, d: Duration) {
        self.queue_wait_ms_total
            .fetch_add(d.as_millis() as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            schema_version: METRICS_SCHEMA_VERSION,
            ops_started: self.ops_started.load(Ordering::Relaxed),
            ops_completed: self.ops_completed.load(Ordering::Relaxed),
            ops_failed: self.ops_failed.load(Ordering::Relaxed),
            ops_cancelled: self.ops_cancelled.load(Ordering::Relaxed),
            ops_deadline: self.ops_deadline.load(Ordering::Relaxed),
            queue_wait_ms_total: self.queue_wait_ms_total.load(Ordering::Relaxed),
            inference_ms_total: self.inference_ms_total.load(Ordering::Relaxed),
            model_loads: self.model_loads.load(Ordering::Relaxed),
            remote_errors: self.remote_errors.load(Ordering::Relaxed),
            busy_rejections: self.busy_rejections.load(Ordering::Relaxed),
            overload_rejections: self.overload_rejections.load(Ordering::Relaxed),
        }
    }
}

/// Serializable metrics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub schema_version: u32,
    pub ops_started: u64,
    pub ops_completed: u64,
    pub ops_failed: u64,
    pub ops_cancelled: u64,
    pub ops_deadline: u64,
    pub queue_wait_ms_total: u64,
    pub inference_ms_total: u64,
    pub model_loads: u64,
    pub remote_errors: u64,
    pub busy_rejections: u64,
    pub overload_rejections: u64,
}

/// Redacted diagnostic bundle for support / `aurum doctor --json` enrichment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticBundle {
    pub schema_version: u32,
    pub request_id: Option<String>,
    pub operation: Option<String>,
    pub metrics: MetricsSnapshot,
    /// Never contains API keys or raw audio.
    pub notes: Vec<String>,
}

impl DiagnosticBundle {
    pub fn from_metrics(metrics: &Metrics) -> Self {
        Self {
            schema_version: METRICS_SCHEMA_VERSION,
            request_id: None,
            operation: None,
            metrics: metrics.snapshot(),
            notes: vec![
                "payloads (PCM/text/API keys) are never included".into(),
                "counters are process-local best-effort".into(),
            ],
        }
    }
}

/// Simple wall-clock timer for inference spans.
#[derive(Debug)]
pub struct SpanTimer {
    start: Instant,
}

impl SpanTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

fn process_metrics_arc() -> Arc<Metrics> {
    use once_cell::sync::Lazy;
    static SHARED: Lazy<Arc<Metrics>> = Lazy::new(|| Arc::new(Metrics::new()));
    Arc::clone(&SHARED)
}

/// Process-global metrics (shared by CLI/FFI).
pub fn process_metrics() -> Arc<Metrics> {
    process_metrics_arc()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_roundtrip() {
        let m = Metrics::new();
        m.record_start();
        m.record_complete(Duration::from_millis(12));
        let s = m.snapshot();
        assert_eq!(s.ops_started, 1);
        assert_eq!(s.ops_completed, 1);
        assert!(s.inference_ms_total >= 12);
        let bundle = DiagnosticBundle::from_metrics(&m);
        assert!(serde_json::to_string(&bundle)
            .unwrap()
            .contains("ops_started"));
    }
}
