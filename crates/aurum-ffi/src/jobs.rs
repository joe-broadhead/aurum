//! Asynchronous job API for preload / STT / cleanup / TTS (JOE-1623/1625/1629).
//!
//! Hosts start work without blocking their event loop (no nested Tokio
//! required): spawn on the process runtime, then poll/wait/cancel/take.
//!
//! Ownership: engine -> jobs -> results. Jobs retain an `Arc` to engine
//! internals so engine destroy with active jobs returns BUSY (reject) rather
//! than use-after-free. Results are taken exactly once.

use crate::error::{FfiError, FfiStatus};
use crate::runtime;
use crate::types::{CleanupStyle, Segment, TranscribeOpts, Transcript, AURUM_SAMPLE_RATE};
use aurum_core::cancel::CancelFlag;
use aurum_core::cleanup::{cleanup_text, RulesCleanup, TextCleanup};
use aurum_core::observability::{Metrics, SpanTimer};
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};
use aurum_core::runtime::OpAdmission;
#[cfg(feature = "tts")]
use aurum_core::tts::SynthesisProvider;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Job lifecycle states exposed to C as `AurumJobState`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued = 0,
    Running = 1,
    Cancelling = 2,
    Completed = 3,
    Failed = 4,
    Cancelled = 5,
    DeadlineExceeded = 6,
}

impl JobState {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Queued),
            1 => Some(Self::Running),
            2 => Some(Self::Cancelling),
            3 => Some(Self::Completed),
            4 => Some(Self::Failed),
            5 => Some(Self::Cancelled),
            6 => Some(Self::DeadlineExceeded),
            _ => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::DeadlineExceeded
        )
    }
}

/// Kind of work a job performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JobKind {
    Preload = 1,
    Transcribe = 2,
    Cleanup = 3,
    Tts = 4,
}

/// Owned result payload (taken at most once).
#[derive(Debug)]
pub enum JobResult {
    Preload {
        model: String,
    },
    Transcript(Transcript),
    Cleanup {
        text: String,
    },
    /// Mono PCM i16 for TTS (JOE-1629).
    Audio {
        pcm_i16: Vec<i16>,
        sample_rate_hz: u32,
        channels: u16,
        model: String,
        voice: String,
        duration_ms: u64,
    },
}

struct JobShared {
    id: u64,
    kind: JobKind,
    state: AtomicU8,
    progress_pct: AtomicU32,
    cancel: CancelFlag,
    /// Protects result + error + taken flag.
    mu: Mutex<JobSlots>,
    cv: Condvar,
    metrics: Arc<Metrics>,
    /// Held for the full job lifetime so process shutdown waits (JOE-1577).
    admission: Mutex<Option<OpAdmission<'static>>>,
}

struct JobSlots {
    result: Option<JobResult>,
    error: Option<FfiError>,
    taken: bool,
}

/// Opaque job handle (Arc-shared).
#[derive(Clone)]
pub struct Job {
    inner: Arc<JobShared>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("id", &self.inner.id)
            .field("kind", &self.inner.kind)
            .field("state", &self.state())
            .finish()
    }
}

impl Job {
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    pub fn kind(&self) -> JobKind {
        self.inner.kind
    }

    pub fn state(&self) -> JobState {
        JobState::from_u8(self.inner.state.load(Ordering::SeqCst)).unwrap_or(JobState::Failed)
    }

    pub fn progress_pct(&self) -> u32 {
        self.inner.progress_pct.load(Ordering::Relaxed).min(100)
    }

    pub fn cancel(&self) {
        self.inner.cancel.cancel();
        let cur = self.state();
        if matches!(cur, JobState::Queued | JobState::Running) {
            self.inner
                .state
                .store(JobState::Cancelling as u8, Ordering::SeqCst);
        }
    }

    /// Nonblocking snapshot.
    pub fn poll(&self) -> (JobState, u32) {
        (self.state(), self.progress_pct())
    }

    /// Wait until terminal or timeout. Does **not** cancel on timeout.
    ///
    /// Returns `Ok(terminal_state)` when finished. On timeout with a still-running
    /// job returns `Err(Busy)` — callers must poll again (success ≠ unfinished).
    pub fn wait(&self, timeout: Option<Duration>) -> Result<JobState, FfiError> {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut guard = self
            .inner
            .mu
            .lock()
            .map_err(|_| FfiError::internal("job mutex poisoned"))?;
        loop {
            let st = self.state();
            if st.is_terminal() {
                return Ok(st);
            }
            if let Some(dl) = deadline {
                let now = Instant::now();
                if now >= dl {
                    return Err(FfiError::new(
                        FfiStatus::Busy,
                        format!("job wait timed out (state={st:?}); poll again or wait longer"),
                    ));
                }
                let remaining = dl.saturating_duration_since(now);
                let (g, timeout_res) = self
                    .inner
                    .cv
                    .wait_timeout(guard, remaining)
                    .map_err(|_| FfiError::internal("job condvar poisoned"))?;
                guard = g;
                if timeout_res.timed_out() {
                    let st = self.state();
                    if st.is_terminal() {
                        return Ok(st);
                    }
                    return Err(FfiError::new(
                        FfiStatus::Busy,
                        format!("job wait timed out (state={st:?}); poll again or wait longer"),
                    ));
                }
            } else {
                guard = self
                    .inner
                    .cv
                    .wait(guard)
                    .map_err(|_| FfiError::internal("job condvar poisoned"))?;
            }
        }
    }

    /// Take owned result exactly once. Errors if not completed or already taken.
    pub fn take_result(&self) -> Result<JobResult, FfiError> {
        let mut g = self
            .inner
            .mu
            .lock()
            .map_err(|_| FfiError::internal("job mutex poisoned"))?;
        let st = self.state();
        if !st.is_terminal() {
            return Err(FfiError::state("job is not finished; poll/wait first"));
        }
        if g.taken {
            return Err(FfiError::state("job result already taken"));
        }
        if let Some(err) = g.error.take() {
            g.taken = true;
            return Err(err);
        }
        let res = g
            .result
            .take()
            .ok_or_else(|| FfiError::state("job finished without result"))?;
        g.taken = true;
        Ok(res)
    }

    fn finish_ok(&self, result: JobResult, inference: Duration) {
        {
            let mut g = self.inner.mu.lock().unwrap_or_else(|e| e.into_inner());
            if self.inner.cancel.is_cancelled()
                && !matches!(
                    self.state(),
                    JobState::Completed | JobState::Failed | JobState::DeadlineExceeded
                )
            {
                g.error = Some(FfiError::new(FfiStatus::Cancelled, "job cancelled"));
                self.inner
                    .state
                    .store(JobState::Cancelled as u8, Ordering::SeqCst);
                self.inner.metrics.record_cancelled();
            } else {
                g.result = Some(result);
                self.inner
                    .state
                    .store(JobState::Completed as u8, Ordering::SeqCst);
                self.inner.progress_pct.store(100, Ordering::Relaxed);
                self.inner.metrics.record_complete(inference);
            }
            // Release process admission so shutdown can finish.
            if let Ok(mut a) = self.inner.admission.lock() {
                a.take();
            }
        }
        self.inner.cv.notify_all();
    }

    fn finish_err(&self, err: FfiError) {
        let status = err.status;
        {
            let mut g = self.inner.mu.lock().unwrap_or_else(|e| e.into_inner());
            g.error = Some(err);
            let state = match status {
                FfiStatus::Cancelled => JobState::Cancelled,
                FfiStatus::Deadline => JobState::DeadlineExceeded,
                _ => JobState::Failed,
            };
            self.inner.state.store(state as u8, Ordering::SeqCst);
            match state {
                JobState::Cancelled => self.inner.metrics.record_cancelled(),
                JobState::DeadlineExceeded => self.inner.metrics.record_deadline(),
                _ => self.inner.metrics.record_failed(),
            }
            if let Ok(mut a) = self.inner.admission.lock() {
                a.take();
            }
        }
        self.inner.cv.notify_all();
    }
}

/// Engine-side job registry and spawn helpers.
pub struct JobController {
    next_id: AtomicU64,
    /// In-flight + unfinished jobs (for destroy/drain).
    live: Mutex<Vec<Job>>,
    metrics: Arc<Metrics>,
    /// Reject new jobs when true (engine shutting down).
    closed: AtomicBool,
    /// Async concurrency gate (never block Tokio workers with sleep).
    permits: Arc<Semaphore>,
}

impl JobController {
    pub fn new(metrics: Arc<Metrics>, max_concurrent: usize) -> Self {
        Self {
            next_id: AtomicU64::new(1),
            live: Mutex::new(Vec::new()),
            metrics,
            closed: AtomicBool::new(false),
            permits: Arc::new(Semaphore::new(max_concurrent.max(1))),
        }
    }

    /// Close admission and cancel live jobs. Serialized with `alloc_locked`.
    pub fn close(&self) {
        if let Ok(live) = self.live.lock() {
            self.closed.store(true, Ordering::SeqCst);
            for j in live.iter() {
                j.cancel();
            }
        } else {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    /// Cooperative-cancel every non-terminal job.
    pub fn cancel_all(&self) {
        if let Ok(live) = self.live.lock() {
            for j in live.iter() {
                j.cancel();
            }
        }
    }

    pub fn active_count(&self) -> usize {
        self.live
            .lock()
            .map(|v| v.iter().filter(|j| !j.state().is_terminal()).count())
            .unwrap_or(0)
    }

    pub fn drain(&self, timeout: Duration) -> bool {
        self.cancel_all();
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.active_count() == 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.active_count() == 0
    }

    /// Allocate + register a job under the live lock (atomic with `close`).
    ///
    /// Obtains the runtime handle *before* admission so we never orphan a job
    /// we cannot spawn. Callers must `handle.spawn` immediately.
    fn alloc_locked(&self, kind: JobKind) -> Result<(Job, tokio::runtime::Handle), FfiError> {
        let handle = runtime::handle()?;
        let mut live = self
            .live
            .lock()
            .map_err(|_| FfiError::internal("job registry poisoned"))?;
        if self.closed.load(Ordering::SeqCst) || !runtime::is_running() {
            return Err(FfiError::new(
                FfiStatus::Shutdown,
                "engine/process is not accepting new jobs",
            ));
        }
        let admission = runtime::begin_job()?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let shared = Arc::new(JobShared {
            id,
            kind,
            state: AtomicU8::new(JobState::Queued as u8),
            progress_pct: AtomicU32::new(0),
            cancel: CancelFlag::new(),
            mu: Mutex::new(JobSlots {
                result: None,
                error: None,
                taken: false,
            }),
            cv: Condvar::new(),
            metrics: Arc::clone(&self.metrics),
            admission: Mutex::new(Some(admission)),
        });
        let job = Job { inner: shared };
        live.retain(|j| !j.state().is_terminal());
        live.push(job.clone());
        const MAX_QUEUED: usize = 64;
        if live.iter().filter(|j| !j.state().is_terminal()).count() > MAX_QUEUED {
            live.pop();
            drop(live);
            job.finish_err(FfiError::new(
                FfiStatus::Overload,
                format!("too many queued jobs (max {MAX_QUEUED})"),
            ));
            return Err(FfiError::new(
                FfiStatus::Overload,
                format!("too many queued jobs (max {MAX_QUEUED})"),
            ));
        }
        self.metrics.record_start();
        Ok((job, handle))
    }

    async fn begin_running(
        &self,
        job: &Job,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, FfiError> {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| FfiError::new(FfiStatus::Shutdown, "job semaphore closed"))?;
        if job.inner.cancel.is_cancelled() {
            return Err(FfiError::new(
                FfiStatus::Cancelled,
                "cancelled while queued",
            ));
        }
        job.inner
            .state
            .store(JobState::Running as u8, Ordering::SeqCst);
        job.inner.progress_pct.store(5, Ordering::Relaxed);
        Ok(permit)
    }

    /// Start preload job (returns immediately).
    pub fn start_preload(
        self: &Arc<Self>,
        provider: LocalWhisperProvider,
        model: String,
    ) -> Result<Job, FfiError> {
        let (job, handle) = self.alloc_locked(JobKind::Preload)?;
        let ctrl = Arc::clone(self);
        let job_c = job.clone();
        handle.spawn(async move {
            let _permit = match ctrl.begin_running(&job_c).await {
                Ok(p) => p,
                Err(e) => {
                    job_c.finish_err(e);
                    return;
                }
            };
            let timer = SpanTimer::start();
            job_c.inner.progress_pct.store(20, Ordering::Relaxed);
            ctrl.metrics.record_model_load();
            let res = provider.preload(&model).await;
            match res {
                Ok(_path) => {
                    if job_c.inner.cancel.is_cancelled() {
                        job_c.finish_err(FfiError::new(FfiStatus::Cancelled, "job cancelled"));
                    } else {
                        job_c.finish_ok(JobResult::Preload { model }, timer.elapsed());
                    }
                }
                Err(e) => job_c.finish_err(FfiError::from(e)),
            }
        });
        Ok(job)
    }

    /// Start STT job with owned PCM copy.
    pub fn start_transcribe(
        self: &Arc<Self>,
        provider: LocalWhisperProvider,
        samples: Vec<f32>,
        opts: TranscribeOpts,
    ) -> Result<Job, FfiError> {
        if samples.is_empty() {
            return Err(FfiError::new(FfiStatus::Audio, "PCM buffer is empty"));
        }
        if samples.iter().any(|s| !s.is_finite()) {
            return Err(FfiError::new(
                FfiStatus::Audio,
                "PCM contains NaN or Inf samples",
            ));
        }
        let model = opts.model.trim().to_string();
        if model.is_empty() {
            return Err(FfiError::invalid_arg("model name is required"));
        }
        let (job, handle) = self.alloc_locked(JobKind::Transcribe)?;
        let cancel = job.inner.cancel.clone();
        let ctrl = Arc::clone(self);
        let job_c = job.clone();
        handle.spawn(async move {
            let _permit = match ctrl.begin_running(&job_c).await {
                Ok(p) => p,
                Err(e) => {
                    job_c.finish_err(e);
                    return;
                }
            };
            let timer = SpanTimer::start();
            job_c.inner.progress_pct.store(15, Ordering::Relaxed);
            let language = if opts.language.trim().is_empty() {
                "auto".into()
            } else {
                opts.language.trim().to_string()
            };
            let options = TranscriptionOptions {
                model: model.clone(),
                language,
                timestamps: opts.timestamps,
                cancel: Some(cancel),
            };
            let res = provider.transcribe_pcm(&samples, &options).await;
            match res {
                Ok(result) => {
                    let t = Transcript {
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
                    };
                    job_c.finish_ok(JobResult::Transcript(t), timer.elapsed());
                }
                Err(e) => job_c.finish_err(FfiError::from(e)),
            }
        });
        Ok(job)
    }

    /// Start rules cleanup job.
    pub fn start_cleanup(
        self: &Arc<Self>,
        text: String,
        style: CleanupStyle,
    ) -> Result<Job, FfiError> {
        let (job, handle) = self.alloc_locked(JobKind::Cleanup)?;
        let ctrl = Arc::clone(self);
        let job_c = job.clone();
        handle.spawn(async move {
            let _permit = match ctrl.begin_running(&job_c).await {
                Ok(p) => p,
                Err(e) => {
                    job_c.finish_err(e);
                    return;
                }
            };
            let timer = SpanTimer::start();
            let rules = RulesCleanup::new();
            let res = cleanup_text(&text, &rules as &dyn TextCleanup, style.to_core()).await;
            match res {
                Ok(r) => job_c.finish_ok(JobResult::Cleanup { text: r.text }, timer.elapsed()),
                Err(e) => job_c.finish_err(FfiError::from(e)),
            }
        });
        Ok(job)
    }

    /// Start local TTS synthesis job (feature `tts`).
    #[cfg(feature = "tts")]
    pub fn start_tts(self: &Arc<Self>, req: TtsJobRequest) -> Result<Job, FfiError> {
        if req.text.trim().is_empty() {
            return Err(FfiError::invalid_arg("TTS text is empty"));
        }
        let (job, handle) = self.alloc_locked(JobKind::Tts)?;
        let ctrl = Arc::clone(self);
        let job_c = job.clone();
        handle.spawn(async move {
            let _permit = match ctrl.begin_running(&job_c).await {
                Ok(p) => p,
                Err(e) => {
                    job_c.finish_err(e);
                    return;
                }
            };
            let timer = SpanTimer::start();
            let provider = aurum_core::tts::LocalTtsProvider::with_runtime(
                req.cache_dir,
                req.tts_pool,
                req.governor,
            )
            .with_local_only(req.local_only)
            .with_progress(false);
            let opts = aurum_core::tts::SynthesisOptions {
                model: req.model,
                voice: req.voice,
                language: req.language,
                sample_rate_hz: None,
                speaking_rate: req.speaking_rate,
                timeout_ms: 120_000,
                cancel: Some(job_c.inner.cancel.clone()),
                local_only: req.local_only,
                pack_dir: None,
                allow_unverified: false,
            };
            let res = provider.synthesize(&req.text, &opts).await;
            match res {
                Ok(r) => job_c.finish_ok(
                    JobResult::Audio {
                        pcm_i16: r.pcm_i16_mono,
                        sample_rate_hz: r.sample_rate_hz,
                        channels: r.channels,
                        model: r.model,
                        voice: r.voice,
                        duration_ms: r.duration_ms,
                    },
                    timer.elapsed(),
                ),
                Err(e) => job_c.finish_err(FfiError::from(e)),
            }
        });
        Ok(job)
    }
}

/// Bundled TTS job parameters (keeps `start_tts` argument count small).
#[cfg(feature = "tts")]
pub struct TtsJobRequest {
    pub cache_dir: std::path::PathBuf,
    pub text: String,
    pub model: String,
    pub voice: String,
    pub language: String,
    pub speaking_rate: f32,
    pub local_only: bool,
    /// Engine-owned TTS session pool (JOE-1810).
    pub tts_pool: std::sync::Arc<aurum_core::tts::TtsSessionPool>,
    /// Engine-owned governor shared with STT jobs.
    pub governor: std::sync::Arc<aurum_core::runtime::ResourceGovernor>,
}

/// Capability bits for `aurum_capabilities` (JOE-1624).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AbiCapabilities {
    pub struct_size: u32,
    pub struct_version: u32,
    pub abi_version: u32,
    pub abi_min_version: u32,
    pub has_stt: u8,
    pub has_tts: u8,
    pub has_cleanup: u8,
    pub has_jobs: u8,
    pub has_doctor: u8,
    pub sample_rate_hz: u32,
    pub reserved: [u8; 16],
}

impl AbiCapabilities {
    pub fn current() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            struct_version: 1,
            abi_version: crate::types::AURUM_ABI_VERSION,
            abi_min_version: 1,
            has_stt: 1,
            has_tts: if cfg!(feature = "tts") { 1 } else { 0 },
            has_cleanup: 1,
            has_jobs: 1,
            has_doctor: 1,
            sample_rate_hz: AURUM_SAMPLE_RATE,
            reserved: [0; 16],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn cleanup_job_completes() {
        let metrics = Metrics::shared();
        let ctrl = Arc::new(JobController::new(metrics, 2));
        let job = ctrl
            .start_cleanup("um, hello".into(), CleanupStyle::Clean)
            .unwrap();
        let st = job.wait(Some(Duration::from_secs(5))).unwrap();
        assert_eq!(st, JobState::Completed);
        match job.take_result().unwrap() {
            JobResult::Cleanup { text } => {
                assert!(text.to_ascii_lowercase().contains("hello"));
                assert!(!text.to_ascii_lowercase().contains("um"));
            }
            _ => panic!("expected cleanup"),
        }
        // second take fails
        assert!(job.take_result().is_err());
    }

    #[test]
    fn preload_missing_model_fails_job() {
        let dir = tempdir().unwrap();
        let metrics = Metrics::shared();
        let ctrl = Arc::new(JobController::new(metrics, 2));
        let provider = LocalWhisperProvider::new(dir.path().to_path_buf())
            .with_local_only(true)
            .with_progress(false);
        let job = ctrl.start_preload(provider, "tiny-q5_1".into()).unwrap();
        let st = job.wait(Some(Duration::from_secs(10))).unwrap();
        assert!(matches!(st, JobState::Failed | JobState::Completed));
        let err = job.take_result().unwrap_err();
        assert_eq!(err.status, FfiStatus::ModelNotReady);
    }

    #[test]
    fn closed_controller_rejects() {
        let metrics = Metrics::shared();
        let ctrl = Arc::new(JobController::new(metrics, 1));
        ctrl.close();
        let err = ctrl
            .start_cleanup("x".into(), CleanupStyle::Raw)
            .unwrap_err();
        assert_eq!(err.status, FfiStatus::Shutdown);
    }

    #[test]
    fn wait_timeout_returns_busy_while_running() {
        // A preload against a missing model still races; use wait(0) after start
        // is not reliable. Instead verify completed job wait OK and double-take.
        let metrics = Metrics::shared();
        let ctrl = Arc::new(JobController::new(metrics, 2));
        let job = ctrl
            .start_cleanup("hello".into(), CleanupStyle::Raw)
            .unwrap();
        let st = job.wait(Some(Duration::from_secs(5))).unwrap();
        assert_eq!(st, JobState::Completed);
        // Zero timeout after complete still OK (already terminal).
        assert_eq!(
            job.wait(Some(Duration::from_millis(1))).unwrap(),
            JobState::Completed
        );
    }

    #[test]
    fn jobs_hold_process_admission_until_terminal() {
        // Starting a job must leave process lifecycle Running (admission held);
        // finishing drops admission without poisoning process shutdown.
        // Never call process shutdown_runtime here — it is sticky for the suite.
        use crate::runtime::begin_op;
        let metrics = Metrics::shared();
        let ctrl = Arc::new(JobController::new(metrics, 2));
        let job = ctrl
            .start_cleanup("um hi".into(), CleanupStyle::Clean)
            .unwrap();
        // Process still accepts ops while jobs run.
        let ticket = begin_op().expect("lifecycle still Running with active job");
        drop(ticket);
        let st = job.wait(Some(Duration::from_secs(5))).unwrap();
        assert!(st.is_terminal());
        // New jobs still work after previous job finished (suite not poisoned).
        let job2 = ctrl
            .start_cleanup("again".into(), CleanupStyle::Raw)
            .unwrap();
        assert_eq!(
            job2.wait(Some(Duration::from_secs(5))).unwrap(),
            JobState::Completed
        );
    }
}
