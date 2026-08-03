//! Privacy-safe per-operation metrics, events, and diagnostics (JOE-1627 / JOE-2222).
//!
//! Counters are process-local (or engine-local via [`Arc`]) and never store
//! payload text, PCM, API keys, or absolute private paths. Hosts may attach a
//! bounded event sink; the default is a no-op with minimal overhead.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Schema version for metrics / diagnostic / event JSON.
pub const METRICS_SCHEMA_VERSION: u32 = 2;
/// Operation event schema version.
pub const OP_EVENT_SCHEMA_VERSION: u32 = 1;
/// Default max buffered events per sink (overflow drops, never blocks).
pub const DEFAULT_EVENT_QUEUE_CAP: usize = 256;

// ---------------------------------------------------------------------------
// Controlled enums (no free-form user labels)
// ---------------------------------------------------------------------------

/// High-level operation class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Stt,
    Tts,
    Cleanup,
    ModelLoad,
    Download,
    BatchItem,
}

impl OpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stt => "stt",
            Self::Tts => "tts",
            Self::Cleanup => "cleanup",
            Self::ModelLoad => "model_load",
            Self::Download => "download",
            Self::BatchItem => "batch_item",
        }
    }
}

/// Controlled stage labels (not free-form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpStage {
    Start,
    QueueWait,
    Admitted,
    ModelLoad,
    Encode,
    NetworkSend,
    NetworkBody,
    Inference,
    Normalize,
    Cleanup,
    Chunk,
    Stitch,
    Output,
    Terminal,
}

/// Terminal outcome category (exactly one per operation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalCategory {
    Completed,
    Failed,
    Cancelled,
    Deadline,
    Overload,
    Busy,
}

impl TerminalCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Deadline => "deadline",
            Self::Overload => "overload",
            Self::Busy => "busy",
        }
    }
}

/// Scope for metrics isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MetricsScope {
    EngineLocal,
    #[default]
    ProcessGlobal,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Versioned privacy-safe operation event (no payloads/secrets/paths).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpEvent {
    pub schema_version: u32,
    pub request_id: u64,
    pub operation: OpKind,
    pub stage: OpStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub scope: MetricsScope,
    /// Monotonic elapsed ms since operation start (when known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoded_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_category: Option<String>,
}

impl OpEvent {
    pub fn stage(request_id: u64, operation: OpKind, stage: OpStage, scope: MetricsScope) -> Self {
        Self {
            schema_version: OP_EVENT_SCHEMA_VERSION,
            request_id,
            operation,
            stage,
            provider_id: None,
            backend_class: None,
            model_id: None,
            scope,
            elapsed_ms: None,
            queue_ms: None,
            encoded_bytes: None,
            decoded_bytes: None,
            chunk_index: None,
            chunk_count: None,
            cache_state: None,
            terminal: None,
            retryable: None,
            error_category: None,
        }
    }

    pub fn with_provider(mut self, id: impl Into<String>) -> Self {
        self.provider_id = Some(id.into());
        self
    }

    pub fn with_model(mut self, id: impl Into<String>) -> Self {
        self.model_id = Some(id.into());
        self
    }

    pub fn with_elapsed_ms(mut self, ms: u64) -> Self {
        self.elapsed_ms = Some(ms);
        self
    }

    pub fn with_terminal(mut self, cat: TerminalCategory, retryable: bool) -> Self {
        self.stage = OpStage::Terminal;
        self.terminal = Some(cat);
        self.retryable = Some(retryable);
        self
    }

    /// Scan serialized form for forbidden markers (privacy canary helper).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

// ---------------------------------------------------------------------------
// Event sink
// ---------------------------------------------------------------------------

/// Host-facing event sink. Must not block core locks or retain payloads.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: OpEvent);
}

/// No-op sink (default).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: OpEvent) {}
}

/// Bounded in-memory queue; overflow increments `dropped` and never blocks.
#[derive(Debug)]
pub struct BoundedEventSink {
    cap: usize,
    inner: Mutex<VecDeque<OpEvent>>,
    dropped: AtomicU64,
}

impl BoundedEventSink {
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            inner: Mutex::new(VecDeque::with_capacity(cap.min(64))),
            dropped: AtomicU64::new(0),
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn drain(&self) -> Vec<OpEvent> {
        self.inner
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|q| q.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for BoundedEventSink {
    fn default() -> Self {
        Self::new(DEFAULT_EVENT_QUEUE_CAP)
    }
}

impl EventSink for BoundedEventSink {
    fn emit(&self, event: OpEvent) {
        if let Ok(mut q) = self.inner.lock() {
            if q.len() >= self.cap {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                return;
            }
            q.push_back(event);
        } else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// Process-wide or engine-local metrics sink (also attachable via [`Arc`]).
pub struct Metrics {
    pub ops_started: AtomicU64,
    pub ops_completed: AtomicU64,
    pub ops_failed: AtomicU64,
    pub ops_cancelled: AtomicU64,
    pub ops_deadline: AtomicU64,
    pub queue_wait_ms_total: AtomicU64,
    pub inference_ms_total: AtomicU64,
    pub encode_ms_total: AtomicU64,
    pub network_ms_total: AtomicU64,
    pub model_loads: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub cache_evictions: AtomicU64,
    pub remote_errors: AtomicU64,
    pub remote_auth_errors: AtomicU64,
    pub remote_rate_limits: AtomicU64,
    pub remote_quota_errors: AtomicU64,
    pub remote_network_errors: AtomicU64,
    pub remote_invalid_payload: AtomicU64,
    pub upload_bytes_total: AtomicU64,
    pub response_bytes_total: AtomicU64,
    pub decoded_bytes_total: AtomicU64,
    pub long_form_chunks_attempted: AtomicU64,
    pub long_form_chunks_completed: AtomicU64,
    pub long_form_chunks_failed: AtomicU64,
    pub tts_chunks_total: AtomicU64,
    pub tts_chars_total: AtomicU64,
    pub batch_items_succeeded: AtomicU64,
    pub batch_items_failed: AtomicU64,
    pub batch_stale_reprocess: AtomicU64,
    pub output_tx_success: AtomicU64,
    pub output_tx_failure: AtomicU64,
    pub busy_rejections: AtomicU64,
    pub overload_rejections: AtomicU64,
    pub events_dropped: AtomicU64,
    /// Optional host sink (outside hot locks when possible).
    event_sink: Mutex<Option<Arc<dyn EventSink>>>,
    scope: MetricsScope,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metrics")
            .field("scope", &self.scope)
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            ops_started: AtomicU64::new(0),
            ops_completed: AtomicU64::new(0),
            ops_failed: AtomicU64::new(0),
            ops_cancelled: AtomicU64::new(0),
            ops_deadline: AtomicU64::new(0),
            queue_wait_ms_total: AtomicU64::new(0),
            inference_ms_total: AtomicU64::new(0),
            encode_ms_total: AtomicU64::new(0),
            network_ms_total: AtomicU64::new(0),
            model_loads: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            cache_evictions: AtomicU64::new(0),
            remote_errors: AtomicU64::new(0),
            remote_auth_errors: AtomicU64::new(0),
            remote_rate_limits: AtomicU64::new(0),
            remote_quota_errors: AtomicU64::new(0),
            remote_network_errors: AtomicU64::new(0),
            remote_invalid_payload: AtomicU64::new(0),
            upload_bytes_total: AtomicU64::new(0),
            response_bytes_total: AtomicU64::new(0),
            decoded_bytes_total: AtomicU64::new(0),
            long_form_chunks_attempted: AtomicU64::new(0),
            long_form_chunks_completed: AtomicU64::new(0),
            long_form_chunks_failed: AtomicU64::new(0),
            tts_chunks_total: AtomicU64::new(0),
            tts_chars_total: AtomicU64::new(0),
            batch_items_succeeded: AtomicU64::new(0),
            batch_items_failed: AtomicU64::new(0),
            batch_stale_reprocess: AtomicU64::new(0),
            output_tx_success: AtomicU64::new(0),
            output_tx_failure: AtomicU64::new(0),
            busy_rejections: AtomicU64::new(0),
            overload_rejections: AtomicU64::new(0),
            events_dropped: AtomicU64::new(0),
            event_sink: Mutex::new(None),
            scope: MetricsScope::ProcessGlobal,
        }
    }

    pub fn engine_local() -> Self {
        let mut m = Self::new();
        m.scope = MetricsScope::EngineLocal;
        m
    }

    pub fn scope(&self) -> MetricsScope {
        self.scope
    }

    /// Process-global shared metrics sink (same instance as [`process_metrics`]).
    pub fn shared() -> Arc<Self> {
        process_metrics_arc()
    }

    pub fn set_event_sink(&self, sink: Option<Arc<dyn EventSink>>) {
        if let Ok(mut g) = self.event_sink.lock() {
            *g = sink;
        }
    }

    pub fn emit(&self, event: OpEvent) {
        // Clone the Arc under the lock, then invoke the host callback *outside*
        // so a slow/re-entrant sink cannot hold the metrics mutex (v0.0.23).
        let sink = self
            .event_sink
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(Arc::clone));
        if let Some(sink) = sink {
            sink.emit(event);
        }
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

    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
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

    pub fn record_remote_error(&self) {
        self.remote_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_terminal(&self, cat: TerminalCategory) {
        match cat {
            TerminalCategory::Completed => {
                // completed also recorded via record_complete in many paths
            }
            TerminalCategory::Failed => self.record_failed(),
            TerminalCategory::Cancelled => self.record_cancelled(),
            TerminalCategory::Deadline => self.record_deadline(),
            TerminalCategory::Overload => self.record_overload(),
            TerminalCategory::Busy => self.record_busy(),
        }
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            schema_version: METRICS_SCHEMA_VERSION,
            scope: self.scope,
            ops_started: self.ops_started.load(Ordering::Relaxed),
            ops_completed: self.ops_completed.load(Ordering::Relaxed),
            ops_failed: self.ops_failed.load(Ordering::Relaxed),
            ops_cancelled: self.ops_cancelled.load(Ordering::Relaxed),
            ops_deadline: self.ops_deadline.load(Ordering::Relaxed),
            queue_wait_ms_total: self.queue_wait_ms_total.load(Ordering::Relaxed),
            inference_ms_total: self.inference_ms_total.load(Ordering::Relaxed),
            encode_ms_total: self.encode_ms_total.load(Ordering::Relaxed),
            network_ms_total: self.network_ms_total.load(Ordering::Relaxed),
            model_loads: self.model_loads.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            cache_evictions: self.cache_evictions.load(Ordering::Relaxed),
            remote_errors: self.remote_errors.load(Ordering::Relaxed),
            remote_auth_errors: self.remote_auth_errors.load(Ordering::Relaxed),
            remote_rate_limits: self.remote_rate_limits.load(Ordering::Relaxed),
            remote_quota_errors: self.remote_quota_errors.load(Ordering::Relaxed),
            remote_network_errors: self.remote_network_errors.load(Ordering::Relaxed),
            remote_invalid_payload: self.remote_invalid_payload.load(Ordering::Relaxed),
            upload_bytes_total: self.upload_bytes_total.load(Ordering::Relaxed),
            response_bytes_total: self.response_bytes_total.load(Ordering::Relaxed),
            decoded_bytes_total: self.decoded_bytes_total.load(Ordering::Relaxed),
            long_form_chunks_attempted: self.long_form_chunks_attempted.load(Ordering::Relaxed),
            long_form_chunks_completed: self.long_form_chunks_completed.load(Ordering::Relaxed),
            long_form_chunks_failed: self.long_form_chunks_failed.load(Ordering::Relaxed),
            tts_chunks_total: self.tts_chunks_total.load(Ordering::Relaxed),
            tts_chars_total: self.tts_chars_total.load(Ordering::Relaxed),
            batch_items_succeeded: self.batch_items_succeeded.load(Ordering::Relaxed),
            batch_items_failed: self.batch_items_failed.load(Ordering::Relaxed),
            batch_stale_reprocess: self.batch_stale_reprocess.load(Ordering::Relaxed),
            output_tx_success: self.output_tx_success.load(Ordering::Relaxed),
            output_tx_failure: self.output_tx_failure.load(Ordering::Relaxed),
            busy_rejections: self.busy_rejections.load(Ordering::Relaxed),
            overload_rejections: self.overload_rejections.load(Ordering::Relaxed),
            events_dropped: self.events_dropped.load(Ordering::Relaxed),
        }
    }
}

/// Serializable metrics snapshot (no averages labelled as p95).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub schema_version: u32,
    #[serde(default = "default_process_global")]
    pub scope: MetricsScope,
    pub ops_started: u64,
    pub ops_completed: u64,
    pub ops_failed: u64,
    pub ops_cancelled: u64,
    pub ops_deadline: u64,
    pub queue_wait_ms_total: u64,
    pub inference_ms_total: u64,
    #[serde(default)]
    pub encode_ms_total: u64,
    #[serde(default)]
    pub network_ms_total: u64,
    pub model_loads: u64,
    #[serde(default)]
    pub cache_hits: u64,
    #[serde(default)]
    pub cache_misses: u64,
    #[serde(default)]
    pub cache_evictions: u64,
    pub remote_errors: u64,
    #[serde(default)]
    pub remote_auth_errors: u64,
    #[serde(default)]
    pub remote_rate_limits: u64,
    #[serde(default)]
    pub remote_quota_errors: u64,
    #[serde(default)]
    pub remote_network_errors: u64,
    #[serde(default)]
    pub remote_invalid_payload: u64,
    #[serde(default)]
    pub upload_bytes_total: u64,
    #[serde(default)]
    pub response_bytes_total: u64,
    #[serde(default)]
    pub decoded_bytes_total: u64,
    #[serde(default)]
    pub long_form_chunks_attempted: u64,
    #[serde(default)]
    pub long_form_chunks_completed: u64,
    #[serde(default)]
    pub long_form_chunks_failed: u64,
    #[serde(default)]
    pub tts_chunks_total: u64,
    #[serde(default)]
    pub tts_chars_total: u64,
    #[serde(default)]
    pub batch_items_succeeded: u64,
    #[serde(default)]
    pub batch_items_failed: u64,
    #[serde(default)]
    pub batch_stale_reprocess: u64,
    #[serde(default)]
    pub output_tx_success: u64,
    #[serde(default)]
    pub output_tx_failure: u64,
    pub busy_rejections: u64,
    pub overload_rejections: u64,
    #[serde(default)]
    pub events_dropped: u64,
}

fn default_process_global() -> MetricsScope {
    MetricsScope::ProcessGlobal
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
                "counters are process-local or engine-local best-effort".into(),
                "totals are sums; averages are not p95".into(),
            ],
        }
    }

    pub fn with_request_id(mut self, id: u64) -> Self {
        self.request_id = Some(id.to_string());
        self
    }
}

// ---------------------------------------------------------------------------
// Terminal guard (exactly one outcome)
// ---------------------------------------------------------------------------

/// Ensures a terminal outcome is recorded at most once (including drop).
pub struct TerminalGuard {
    metrics: Arc<Metrics>,
    request_id: u64,
    operation: OpKind,
    scope: MetricsScope,
    finished: bool,
    start: Instant,
}

impl TerminalGuard {
    pub fn start(metrics: Arc<Metrics>, request_id: u64, operation: OpKind) -> Self {
        metrics.record_start();
        let scope = metrics.scope();
        metrics.emit(OpEvent::stage(request_id, operation, OpStage::Start, scope));
        Self {
            metrics,
            request_id,
            operation,
            scope,
            finished: false,
            start: Instant::now(),
        }
    }

    pub fn finish(&mut self, cat: TerminalCategory, retryable: bool) {
        if self.finished {
            return;
        }
        self.finished = true;
        match cat {
            TerminalCategory::Completed => {
                self.metrics.record_complete(self.start.elapsed());
            }
            other => self.metrics.record_terminal(other),
        }
        self.metrics.emit(
            OpEvent::stage(
                self.request_id,
                self.operation,
                OpStage::Terminal,
                self.scope,
            )
            .with_elapsed_ms(self.start.elapsed().as_millis() as u64)
            .with_terminal(cat, retryable),
        );
    }

    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Panic / early return: count as failed once.
        if !self.finished {
            self.finish(TerminalCategory::Failed, false);
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

/// Forbidden substrings that must never appear in public observability output.
pub const PRIVACY_CANARY_MARKERS: &[&str] = &[
    "sk-test-secret-key",
    "BEGIN_PRIVATE_AUDIO",
    "USER_TRANSCRIPT_PAYLOAD",
    "SYNTHESIS_TEXT_SECRET",
    "/Users/private/home/",
    "Authorization: Bearer",
];

/// Scan JSON/text for privacy canary markers.
pub fn privacy_scan(text: &str) -> Result<(), String> {
    for m in PRIVACY_CANARY_MARKERS {
        if text.contains(m) {
            return Err(format!("privacy canary hit: marker present: {m}"));
        }
    }
    Ok(())
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
        assert_eq!(s.schema_version, METRICS_SCHEMA_VERSION);
        let bundle = DiagnosticBundle::from_metrics(&m);
        assert!(serde_json::to_string(&bundle)
            .unwrap()
            .contains("ops_started"));
    }

    #[test]
    fn terminal_guard_once() {
        let m = Arc::new(Metrics::engine_local());
        let sink = Arc::new(BoundedEventSink::new(32));
        m.set_event_sink(Some(sink.clone()));
        {
            let mut g = TerminalGuard::start(m.clone(), 42, OpKind::Stt);
            g.finish(TerminalCategory::Completed, false);
            g.finish(TerminalCategory::Failed, false); // no-op
        }
        assert_eq!(m.snapshot().ops_started, 1);
        assert_eq!(m.snapshot().ops_completed, 1);
        assert_eq!(m.snapshot().ops_failed, 0);
        let events = sink.drain();
        assert!(events.iter().any(|e| e.stage == OpStage::Start));
        assert_eq!(events.iter().filter(|e| e.terminal.is_some()).count(), 1);
        assert!(events.iter().all(|e| e.request_id == 42));
    }

    #[test]
    fn terminal_guard_drop_counts_failed() {
        let m = Arc::new(Metrics::new());
        {
            let _g = TerminalGuard::start(m.clone(), 1, OpKind::Tts);
        }
        assert_eq!(m.snapshot().ops_failed, 1);
    }

    #[test]
    fn bounded_sink_drops() {
        let sink = BoundedEventSink::new(2);
        for i in 0..5 {
            sink.emit(OpEvent::stage(
                i,
                OpKind::Stt,
                OpStage::Start,
                MetricsScope::ProcessGlobal,
            ));
        }
        assert_eq!(sink.len(), 2);
        assert_eq!(sink.dropped(), 3);
    }

    #[test]
    fn emit_does_not_hold_metrics_mutex_during_sink_callback() {
        use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
        use std::sync::Mutex as StdMutex;

        /// Sink that re-enters Metrics::set_event_sink while handling emit.
        struct ReentrantSink {
            metrics: Arc<Metrics>,
            hit: Arc<AtomicBool>,
            // Keep a strong ref so Drop of prior sink does not race.
            _guard: StdMutex<()>,
        }

        impl EventSink for ReentrantSink {
            fn emit(&self, _event: OpEvent) {
                self.hit.store(true, AtomicOrdering::SeqCst);
                // Would deadlock if Metrics::emit held event_sink while calling us.
                self.metrics.set_event_sink(None);
            }
        }

        let m = Arc::new(Metrics::new());
        let hit = Arc::new(AtomicBool::new(false));
        let sink: Arc<dyn EventSink> = Arc::new(ReentrantSink {
            metrics: Arc::clone(&m),
            hit: Arc::clone(&hit),
            _guard: StdMutex::new(()),
        });
        m.set_event_sink(Some(sink));
        m.emit(OpEvent::stage(
            1,
            OpKind::Stt,
            OpStage::Start,
            MetricsScope::EngineLocal,
        ));
        assert!(hit.load(AtomicOrdering::SeqCst));
    }

    #[test]
    fn distinct_outcomes() {
        let m = Metrics::new();
        m.record_cancelled();
        m.record_deadline();
        m.record_overload();
        m.record_busy();
        m.record_failed();
        let s = m.snapshot();
        assert_eq!(s.ops_cancelled, 1);
        assert_eq!(s.ops_deadline, 1);
        assert_eq!(s.overload_rejections, 1);
        assert_eq!(s.busy_rejections, 1);
        assert_eq!(s.ops_failed, 1);
    }

    #[test]
    fn engine_vs_process_scope() {
        let eng = Metrics::engine_local();
        let proc = Metrics::new();
        assert_eq!(eng.scope(), MetricsScope::EngineLocal);
        assert_eq!(proc.scope(), MetricsScope::ProcessGlobal);
        eng.record_start();
        assert_eq!(eng.snapshot().ops_started, 1);
        assert_eq!(proc.snapshot().ops_started, 0);
    }

    #[test]
    fn privacy_canary_on_event_and_snapshot() {
        let m = Metrics::new();
        m.record_start();
        let event = OpEvent::stage(
            7,
            OpKind::Stt,
            OpStage::Inference,
            MetricsScope::EngineLocal,
        )
        .with_provider("local")
        .with_model("base");
        let json = event.to_json().unwrap();
        privacy_scan(&json).unwrap();
        privacy_scan(&serde_json::to_string(&m.snapshot()).unwrap()).unwrap();
        // Inject markers into notes would fail — bundle notes are fixed.
        let mut bad = json;
        bad.push_str("sk-test-secret-key");
        assert!(privacy_scan(&bad).is_err());
    }

    #[test]
    fn event_schema_stable() {
        let e = OpEvent::stage(
            1,
            OpKind::Cleanup,
            OpStage::Normalize,
            MetricsScope::ProcessGlobal,
        );
        let j1 = e.to_json().unwrap();
        let j2 = e.to_json().unwrap();
        assert_eq!(j1, j2);
        assert!(j1.contains("schema_version"));
    }
}
