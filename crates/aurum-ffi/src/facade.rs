//! Pure Rust façade — single source of behavior for C (and future UniFFI).
//!
//! ## Ownership (JOE-1622 / JOE-1625 / JOE-1810)
//!
//! * Each [`Engine`] owns **engine-local** STT/TTS model pools, a resource
//!   governor, cache policy, exclusive busy state, cancel publication, metrics
//!   sink, and a [`JobController`] for async jobs.
//! * Jobs and blocking STT/TTS share those pools — not the process-global
//!   residual used by bare `LocalWhisperProvider::new` / CLI non-engine paths.
//! * Process-wide Tokio runtime remains shared; **engine shutdown** drains and
//!   clears **that engine's** pools only and does not poison other engines.
//! * Process [`shutdown`] / [`shutdown_with_timeout`] close admission globally
//!   and clear the process-global STT cache (for non-engine residual users).

use crate::error::{FfiError, FfiStatus};
use crate::jobs::{Job, JobController};
use crate::runtime::{self, ShutdownOutcome};
use crate::types::{
    CleanupStyle, EngineConfig, Segment, TranscribeOpts, Transcript, AURUM_SAMPLE_RATE,
};
use aurum_core::cancel::CancelFlag;
use aurum_core::cleanup::{cleanup_text, RulesCleanup, TextCleanup};
use aurum_core::observability::Metrics;
use aurum_core::providers::local::{clear_context_cache, SttContextPool};
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};
use aurum_core::runtime::{OpAdmission, ResourceGovernor};
#[cfg(feature = "tts")]
use aurum_core::tts::TtsSessionPool;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// RAII: one exported C call is using this engine (JOE-1647).
pub struct ExportGuard<'a> {
    engine: &'a Engine,
}

impl Drop for ExportGuard<'_> {
    fn drop(&mut self) {
        self.engine.export_depth.fetch_sub(1, Ordering::SeqCst);
    }
}

/// RAII guard: sets engine `busy` and process lifecycle active count; clears on
/// drop (including panic unwinds through `block_on`).
struct BusyGuard<'a> {
    busy: &'a AtomicBool,
    _admission: OpAdmission<'static>,
}

impl<'a> BusyGuard<'a> {
    fn acquire(busy: &'a AtomicBool, closed: &AtomicBool, what: &str) -> Result<Self, FfiError> {
        // Reject closed engines before taking process admission (JOE-1647).
        if closed.load(Ordering::SeqCst) {
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "engine is shut down; create a new engine",
            ));
        }
        // Process admission first so shutdown races cannot leave a half-admitted op.
        let admission = runtime::begin_op()?;
        if closed.load(Ordering::SeqCst) {
            drop(admission);
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "engine is shut down; create a new engine",
            ));
        }
        if busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            // Drop admission immediately — engine is busy, not process-rejected.
            drop(admission);
            return Err(FfiError::state(format!(
                "{what} already in progress on this engine (one exclusive op at a time)"
            )));
        }
        // Re-check closed after claiming busy: destroy may have started.
        if closed.load(Ordering::SeqCst) {
            busy.store(false, Ordering::SeqCst);
            drop(admission);
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "engine is shut down; create a new engine",
            ));
        }
        Ok(Self {
            busy,
            _admission: admission,
        })
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::SeqCst);
        // OpAdmission drop unregisters the process active count.
    }
}

/// Local STT/TTS engine handle for embedders (explicit ownership, JOE-1622 / JOE-1810).
pub struct Engine {
    cache_dir: PathBuf,
    local_only: bool,
    /// Engine-local Whisper context residency (not process-global).
    stt_pool: Arc<SttContextPool>,
    #[cfg(feature = "tts")]
    tts_pool: Arc<TtsSessionPool>,
    governor: Arc<ResourceGovernor>,
    provider: LocalWhisperProvider,
    /// Currently published cancel token for the in-flight exclusive op (if any).
    /// Fresh per operation — never reset a shared engine-global flag across ops.
    active_cancel: Mutex<Option<CancelFlag>>,
    /// True while blocking preload or transcribe is in flight on this handle.
    busy: AtomicBool,
    last_error: Mutex<String>,
    /// Async job controller (nonblocking start; poll/wait/cancel/take).
    jobs: Arc<JobController>,
    metrics: Arc<Metrics>,
    /// Engine closed for new jobs (after [`Self::shutdown_engine`]).
    closed: AtomicBool,
    /// Count of in-flight **exported** C calls that still touch this engine
    /// (including post-façade last_error writes). Close/destroy must wait for
    /// this to reach zero (JOE-1647).
    export_depth: AtomicU32,
}

impl Engine {
    /// Create an engine. `cache_dir` must be non-empty.
    pub fn new(config: EngineConfig) -> Result<Self, FfiError> {
        if config.cache_dir.trim().is_empty() {
            return Err(FfiError::invalid_arg(
                "cache_dir is required (host must supply a writable model cache path)",
            ));
        }
        if !runtime::is_running() {
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "process lifecycle is not accepting new engines (call before aurum_shutdown)",
            ));
        }
        let cache_dir = PathBuf::from(config.cache_dir.trim());
        let stt_pool = Arc::new(SttContextPool::new());
        #[cfg(feature = "tts")]
        let tts_pool = Arc::new(TtsSessionPool::new());
        let governor = Arc::new(ResourceGovernor::default());
        let provider = LocalWhisperProvider::with_runtime(
            cache_dir.clone(),
            Arc::clone(&stt_pool),
            Arc::clone(&governor),
        )
        .with_progress(config.progress_logging)
        .with_local_only(config.local_only);
        let metrics = Metrics::shared();
        // Default: up to 2 concurrent jobs (exclusive blocking path still serial).
        let jobs = Arc::new(JobController::new(Arc::clone(&metrics), 2));
        Ok(Self {
            cache_dir,
            local_only: config.local_only,
            stt_pool,
            #[cfg(feature = "tts")]
            tts_pool,
            governor,
            provider,
            active_cancel: Mutex::new(None),
            busy: AtomicBool::new(false),
            last_error: Mutex::new(String::new()),
            jobs,
            metrics,
            closed: AtomicBool::new(false),
            export_depth: AtomicU32::new(0),
        })
    }

    /// Fresh provider handle sharing this engine's STT pool and governor (JOE-1810).
    fn stt_provider_handle(&self, progress: bool) -> LocalWhisperProvider {
        LocalWhisperProvider::with_runtime(
            self.cache_dir.clone(),
            Arc::clone(&self.stt_pool),
            Arc::clone(&self.governor),
        )
        .with_progress(progress)
        .with_local_only(self.local_only)
    }

    /// Clear engine-local model residency after jobs/exclusive work have drained.
    fn clear_engine_pools(&self) {
        self.stt_pool.clear();
        #[cfg(feature = "tts")]
        self.tts_pool.clear();
    }

    /// Enter an exported C call boundary (JOE-1647).
    pub fn begin_export(&self) -> Result<ExportGuard<'_>, FfiError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "engine is shut down; create a new engine",
            ));
        }
        if !runtime::is_running() {
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "process lifecycle is stopped",
            ));
        }
        self.export_depth.fetch_add(1, Ordering::SeqCst);
        // Re-check closed after increment so close cannot free while we are "in".
        if self.closed.load(Ordering::SeqCst) {
            self.export_depth.fetch_sub(1, Ordering::SeqCst);
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "engine is shut down; create a new engine",
            ));
        }
        Ok(ExportGuard { engine: self })
    }

    pub fn export_depth(&self) -> u32 {
        self.export_depth.load(Ordering::SeqCst)
    }

    /// Engine-local metrics snapshot (also feeds process counters).
    pub fn metrics_snapshot(&self) -> aurum_core::observability::MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Drain this engine's jobs and wait for exclusive blocking ops (JOE-1622 / JOE-1647).
    ///
    /// Sets `closed` first so new blocking calls and jobs are rejected. Cancels
    /// the in-flight exclusive op, drains async jobs, then waits until `busy` is
    /// clear **and** `export_depth == 0` (full C boundary has exited), or
    /// `timeout` elapses.
    pub fn shutdown_engine(&self, timeout: Duration) -> Result<(), FfiError> {
        self.closed.store(true, Ordering::SeqCst);
        self.jobs.close();
        // Cancel in-flight exclusive op.
        self.cancel();
        let deadline = std::time::Instant::now() + timeout;
        // Drain jobs with remaining budget.
        let jobs_ok = self.jobs.drain(timeout);
        // Wait for exclusive busy (preload/transcribe) and full export boundary.
        while self.busy.load(Ordering::SeqCst) || self.export_depth.load(Ordering::SeqCst) > 0 {
            if std::time::Instant::now() >= deadline {
                let reason = if self.busy.load(Ordering::SeqCst) {
                    "engine still has an exclusive blocking operation in progress"
                } else {
                    "engine still has an in-flight exported C call (including error-path writes)"
                };
                return Err(FfiError::new(FfiStatus::Busy, reason));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if !jobs_ok {
            return Err(FfiError::new(
                FfiStatus::Busy,
                format!(
                    "engine still has {} active job(s)",
                    self.jobs.active_count()
                ),
            ));
        }
        // Drop engine-local residency only after exclusive work + jobs finished
        // (JOE-1810). Does not touch process-global pools.
        self.clear_engine_pools();
        Ok(())
    }

    /// True while exclusive preload/transcribe is in flight.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// True after shutdown/destroy has closed admission.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Start async preload (returns immediately).
    pub fn start_preload_job(&self, model: &str) -> Result<Job, FfiError> {
        self.ensure_open()?;
        let model = model.trim().to_string();
        if model.is_empty() {
            return Err(FfiError::invalid_arg("model name is required"));
        }
        // Share engine-local STT pool (JOE-1810).
        let provider = self.stt_provider_handle(false);
        self.jobs.start_preload(provider, model)
    }

    /// Start async STT job (copies PCM; returns immediately).
    pub fn start_transcribe_job(
        &self,
        samples: &[f32],
        opts: &TranscribeOpts,
    ) -> Result<Job, FfiError> {
        self.ensure_open()?;
        let provider = self.stt_provider_handle(false);
        self.jobs
            .start_transcribe(provider, samples.to_vec(), opts.clone())
    }

    /// Start async rules cleanup.
    pub fn start_cleanup_job(&self, text: &str, style: CleanupStyle) -> Result<Job, FfiError> {
        self.ensure_open()?;
        self.jobs.start_cleanup(text.to_string(), style)
    }

    /// Start async local TTS job (feature `tts`).
    #[cfg(feature = "tts")]
    pub fn start_tts_job(
        &self,
        text: &str,
        model: &str,
        voice: &str,
        language: &str,
        speaking_rate: f32,
    ) -> Result<Job, FfiError> {
        self.ensure_open()?;
        self.jobs.start_tts(crate::jobs::TtsJobRequest {
            cache_dir: self.cache_dir.clone(),
            text: text.to_string(),
            model: model.to_string(),
            voice: voice.to_string(),
            language: language.to_string(),
            speaking_rate,
            local_only: self.local_only,
            tts_pool: Arc::clone(&self.tts_pool),
            governor: Arc::clone(&self.governor),
        })
    }

    fn ensure_open(&self) -> Result<(), FfiError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "engine is shut down; create a new engine",
            ));
        }
        if !runtime::is_running() {
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "process lifecycle is stopped",
            ));
        }
        Ok(())
    }

    pub fn last_error(&self) -> String {
        self.last_error
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Set last error from the C boundary (e.g. panic → INTERNAL).
    pub fn set_last_error_message(&self, msg: impl Into<String>) {
        if let Ok(mut g) = self.last_error.lock() {
            *g = msg.into();
        }
    }

    fn set_error(&self, msg: impl Into<String>) {
        self.set_last_error_message(msg);
    }

    fn clear_error(&self) {
        if let Ok(mut g) = self.last_error.lock() {
            g.clear();
        }
    }

    fn store_err(&self, err: &FfiError) {
        self.set_error(err.message.clone());
    }

    fn publish_cancel(&self, flag: CancelFlag) {
        if let Ok(mut g) = self.active_cancel.lock() {
            *g = Some(flag);
        }
    }

    fn clear_cancel(&self) {
        if let Ok(mut g) = self.active_cancel.lock() {
            *g = None;
        }
    }

    /// Whether the ggml file is present for `model`.
    pub fn is_model_ready(&self, model: &str) -> bool {
        if model.trim().is_empty() {
            return false;
        }
        self.provider.is_model_cached(model.trim())
    }

    /// Ensure model on disk (unless local_only) and warm the process context cache.
    ///
    /// Exclusive with other ops on this engine (same busy flag as transcribe).
    pub fn preload(&self, model: &str) -> Result<(), FfiError> {
        let model = model.trim();
        if model.is_empty() {
            let e = FfiError::invalid_arg("model name is required");
            self.store_err(&e);
            return Err(e);
        }
        let _guard = match BusyGuard::acquire(&self.busy, &self.closed, "operation") {
            Ok(g) => g,
            Err(e) => {
                self.store_err(&e);
                return Err(e);
            }
        };

        let result = runtime::block_on(async { self.provider.preload(model).await });
        match result {
            Ok(Ok(_)) => {
                self.clear_error();
                Ok(())
            }
            Ok(Err(e)) => {
                let fe = FfiError::from(e);
                self.store_err(&fe);
                Err(fe)
            }
            Err(e) => {
                self.store_err(&e);
                Err(e)
            }
        }
    }

    /// Request cooperative cancel of the in-flight transcription (if any).
    ///
    /// Targets only the currently published job token — never a future op.
    pub fn cancel(&self) {
        if let Ok(g) = self.active_cancel.lock() {
            if let Some(flag) = g.as_ref() {
                flag.cancel();
            }
        }
    }

    /// Transcribe mono PCM at [`AURUM_SAMPLE_RATE`] Hz.
    ///
    /// At most one exclusive op (preload/transcribe) per engine at a time.
    /// Cancel flag is fresh per call. On panic inside inference, busy is still
    /// released via [`BusyGuard`].
    pub fn transcribe_pcm(
        &self,
        samples: &[f32],
        opts: &TranscribeOpts,
    ) -> Result<Transcript, FfiError> {
        let model = opts.model.trim();
        if model.is_empty() {
            let e = FfiError::invalid_arg("model name is required");
            self.store_err(&e);
            return Err(e);
        }
        if samples.is_empty() {
            let e = FfiError::new(FfiStatus::Audio, "PCM buffer is empty");
            self.store_err(&e);
            return Err(e);
        }
        if samples.iter().any(|s| !s.is_finite()) {
            let e = FfiError::new(FfiStatus::Audio, "PCM contains NaN or Inf samples");
            self.store_err(&e);
            return Err(e);
        }

        let _guard = match BusyGuard::acquire(&self.busy, &self.closed, "transcription") {
            Ok(g) => g,
            Err(e) => {
                self.store_err(&e);
                return Err(e);
            }
        };

        let cancel = CancelFlag::new();
        self.publish_cancel(cancel.clone());
        // Clear published token when this scope ends (success, error, or panic).
        struct ClearCancel<'a>(&'a Engine);
        impl Drop for ClearCancel<'_> {
            fn drop(&mut self) {
                self.0.clear_cancel();
            }
        }
        let _clear = ClearCancel(self);

        let language = if opts.language.trim().is_empty() {
            "auto".to_string()
        } else {
            opts.language.trim().to_string()
        };
        let options = TranscriptionOptions {
            model: model.to_string(),
            language,
            timestamps: opts.timestamps,
            cancel: Some(cancel),
        };

        // Synchronous block_on: no *extra* FFI-side buffer copy. Core still copies once
        // inside `from_pcm_slice` so the spawn_blocking worker can own `'static` PCM.
        let outcome =
            runtime::block_on(async { self.provider.transcribe_pcm(samples, &options).await });

        match outcome {
            Ok(Ok(result)) => {
                self.clear_error();
                Ok(Transcript {
                    text: result.text().to_string(),
                    language: result.language().map(|s| s.to_string()),
                    model: result.model().to_string(),
                    duration_secs: result.duration_secs(),
                    timestamps_reliable: result.timestamps_reliable(),
                    segments: result
                        .segments()
                        .iter()
                        .map(|s| Segment {
                            start_s: s.start(),
                            end_s: s.end(),
                            text: s.text().to_string(),
                        })
                        .collect(),
                    cleanup_style: CleanupStyle::Raw,
                })
            }
            Ok(Err(e)) => {
                let fe = FfiError::from(e);
                self.store_err(&fe);
                Err(fe)
            }
            Err(e) => {
                self.store_err(&e);
                Err(e)
            }
        }
    }

    /// Sample rate hosts must use (Hz).
    pub fn sample_rate(&self) -> u32 {
        AURUM_SAMPLE_RATE
    }
}

/// On-device rules cleanup (no network, no engine handle required).
///
/// Does not participate in engine busy / lifecycle active accounting (pure string
/// work; no whisper context). `shutdown`'s drain waits only for preload/transcribe.
pub fn cleanup_rules(text: &str, style: CleanupStyle) -> Result<String, FfiError> {
    let rules = RulesCleanup::new();
    let core_style = style.to_core();
    let outcome = runtime::block_on(async {
        cleanup_text(text, &rules as &dyn TextCleanup, core_style).await
    })?;
    match outcome {
        Ok(r) => Ok(r.text),
        Err(e) => Err(FfiError::from(e)),
    }
}

/// Process-level teardown with a timeout.
///
/// On success (`Ok`), active count is zero and the whisper context cache is cleared.
/// On timeout (`Err` with `Busy`), caches are **not** cleared — native work may still
/// hold contexts. The lifecycle remains ShuttingDown / not accepting new work.
pub fn shutdown_with_timeout(timeout: Duration) -> Result<(), FfiError> {
    match runtime::shutdown_runtime(timeout) {
        ShutdownOutcome::Stopped => {
            clear_context_cache();
            Ok(())
        }
        ShutdownOutcome::Busy { active } => Err(FfiError::new(
            FfiStatus::Busy,
            format!("shutdown timed out with {active} active operation(s); contexts not cleared"),
        )),
    }
}

/// Process-level teardown: drain in-flight engine ops (default timeout), then clear
/// whisper cache only if drain succeeded.
///
/// Hosts must not start new calls after this. Safe to call with no engines left.
/// Prefer destroy engines first; then `shutdown` before process exit (Metal).
///
/// For a status-returning drain with custom timeout, use [`shutdown_with_timeout`].
pub fn shutdown() {
    let _ = shutdown_with_timeout(runtime::DEFAULT_SHUTDOWN_TIMEOUT);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejects_empty_cache_dir() {
        match Engine::new(EngineConfig {
            cache_dir: "  ".into(),
            local_only: true,
            progress_logging: false,
        }) {
            Ok(_) => panic!("expected error"),
            Err(err) => assert_eq!(err.status, FfiStatus::InvalidArg),
        }
    }

    #[test]
    fn cleanup_rules_strips_fillers() {
        let out = cleanup_rules("um, hello there", CleanupStyle::Clean).unwrap();
        assert!(out.to_ascii_lowercase().contains("hello"));
        assert!(!out.to_ascii_lowercase().contains("um"));
    }

    #[test]
    fn cleanup_raw_trims() {
        let out = cleanup_rules("  hi  ", CleanupStyle::Raw).unwrap();
        assert_eq!(out, "hi");
    }

    #[test]
    fn empty_pcm_errors() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        let err = engine
            .transcribe_pcm(
                &[],
                &TranscribeOpts {
                    model: "tiny-q5_1".into(),
                    language: "en".into(),
                    timestamps: false,
                },
            )
            .unwrap_err();
        assert_eq!(err.status, FfiStatus::Audio);
    }

    #[test]
    fn missing_model_local_only() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        assert!(!engine.is_model_ready("tiny-q5_1"));
        let err = engine.preload("tiny-q5_1").unwrap_err();
        assert_eq!(err.status, FfiStatus::ModelNotReady);
    }

    #[test]
    fn requires_model_name() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        let err = engine
            .transcribe_pcm(
                &[0.0; 100],
                &TranscribeOpts {
                    model: "".into(),
                    language: "en".into(),
                    timestamps: false,
                },
            )
            .unwrap_err();
        assert_eq!(err.status, FfiStatus::InvalidArg);
    }

    #[test]
    fn busy_guard_releases_on_error_path() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        // First preload fails (not cached) but must release busy.
        let _ = engine.preload("tiny-q5_1");
        // Second exclusive op must not get permanent STATE from leaked busy.
        let err = engine.preload("tiny-q5_1").unwrap_err();
        assert_eq!(err.status, FfiStatus::ModelNotReady);
        assert!(!engine.busy.load(Ordering::SeqCst));
    }

    #[test]
    fn exclusive_op_rejects_while_busy() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();

        let _g = BusyGuard::acquire(&engine.busy, &engine.closed, "test").unwrap();
        let err = engine
            .transcribe_pcm(
                &[0.0; 100],
                &TranscribeOpts {
                    model: "tiny-q5_1".into(),
                    language: "en".into(),
                    timestamps: false,
                },
            )
            .unwrap_err();
        assert_eq!(err.status, FfiStatus::State);

        let err = engine.preload("tiny-q5_1").unwrap_err();
        assert_eq!(err.status, FfiStatus::State);
    }

    #[test]
    fn distinct_engines_have_independent_busy() {
        let dir = tempdir().unwrap();
        let path = dir.path().display().to_string();
        let a = Engine::new(EngineConfig {
            cache_dir: path.clone(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        let b = Engine::new(EngineConfig {
            cache_dir: path,
            local_only: true,
            progress_logging: false,
        })
        .unwrap();

        let _hold_a = BusyGuard::acquire(&a.busy, &a.closed, "test").unwrap();
        // B must not see STATE from A's exclusive op — only ModelNotReady (empty cache).
        let err = b.preload("tiny-q5_1").unwrap_err();
        assert_eq!(err.status, FfiStatus::ModelNotReady);
        assert!(!b.busy.load(Ordering::SeqCst));
    }

    #[test]
    fn shutdown_rejects_new_blocking_ops() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        engine.shutdown_engine(Duration::from_secs(1)).unwrap();
        let err = engine.preload("tiny-q5_1").unwrap_err();
        assert_eq!(err.status, FfiStatus::Shutdown);
        assert!(engine.is_closed());
    }

    #[test]
    fn cancel_without_active_op_is_noop() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        engine.cancel(); // must not poison a future op
        let err = engine.preload("tiny-q5_1").unwrap_err();
        assert_eq!(err.status, FfiStatus::ModelNotReady);
    }

    #[test]
    fn engines_use_isolated_stt_pools() {
        let dir = tempdir().unwrap();
        let a = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        let b = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        assert_ne!(
            Arc::as_ptr(a.provider.pool()),
            Arc::as_ptr(b.provider.pool())
        );
        assert_ne!(
            Arc::as_ptr(a.provider.pool()),
            Arc::as_ptr(&aurum_core::providers::local::process_global_stt_pool())
        );
        // Process-global residual remains available for non-engine callers.
        assert_eq!(
            Arc::as_ptr(&aurum_core::providers::local::process_global_stt_pool()),
            Arc::as_ptr(&aurum_core::providers::local::process_global_stt_pool())
        );
    }

    #[test]
    fn shutdown_waits_for_export_depth() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        let guard = engine.begin_export().unwrap();
        assert_eq!(engine.export_depth(), 1);
        let err = engine
            .shutdown_engine(Duration::from_millis(60))
            .unwrap_err();
        assert_eq!(err.status, FfiStatus::Busy);
        drop(guard);
        assert_eq!(engine.export_depth(), 0);
        engine.shutdown_engine(Duration::from_secs(1)).unwrap();
    }
}
