//! Process/engine resource governor and overload policy (JOE-1596 / JOE-1831).
//!
//! Permit waiters use a [`Condvar`] so release wakes waiters promptly instead of
//! busy-spinning on a short sleep. Cancel/deadline are re-checked on each wake
//! (and at a short max wait slice so cancel without a release still progresses).

use crate::error::{ProviderError, Result};
use crate::runtime::op_context::OpContext;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Max time a waiter sleeps before re-checking cancel/deadline when no release
/// has notified (cancel flags do not currently signal the condvar).
const WAIT_SLICE: Duration = Duration::from_millis(50);

/// Kind of concurrent work limited by the governor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermitKind {
    ModelLoad,
    LocalStt,
    LocalTts,
    Remote,
    Blocking,
}

/// Configuration for a [`ResourceGovernor`].
#[derive(Debug, Clone)]
pub struct GovernorConfig {
    pub max_model_loads: usize,
    pub max_local_stt: usize,
    pub max_local_tts: usize,
    pub max_remote: usize,
    pub max_blocking: usize,
    /// Total Whisper inference threads across concurrent STT jobs.
    pub max_cpu_threads: usize,
    /// Soft memory reservation budget (bytes).
    pub max_memory_bytes: u64,
    /// Max time to wait for a permit.
    pub queue_timeout: Duration,
    /// When true, fail immediately if a permit is not free (no wait).
    pub fail_fast: bool,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 16);
        Self {
            max_model_loads: 2,
            max_local_stt: 2,
            max_local_tts: 2,
            max_remote: 4,
            max_blocking: 4,
            max_cpu_threads: cpus,
            max_memory_bytes: 2 * 1024 * 1024 * 1024, // 2 GiB soft
            queue_timeout: Duration::from_secs(30),
            fail_fast: false,
        }
    }
}

impl GovernorConfig {
    /// Conservative mobile/low-memory profile.
    pub fn mobile() -> Self {
        Self {
            max_model_loads: 1,
            max_local_stt: 1,
            max_local_tts: 1,
            max_remote: 2,
            max_blocking: 2,
            max_cpu_threads: 2,
            max_memory_bytes: 512 * 1024 * 1024,
            queue_timeout: Duration::from_secs(15),
            fail_fast: false,
        }
    }

    /// Higher-concurrency server profile.
    pub fn server() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8);
        Self {
            max_model_loads: 4,
            max_local_stt: cpus.max(4),
            max_local_tts: 4,
            max_remote: 16,
            max_blocking: cpus.max(8),
            max_cpu_threads: cpus,
            max_memory_bytes: 8 * 1024 * 1024 * 1024,
            queue_timeout: Duration::from_secs(60),
            fail_fast: false,
        }
    }

    /// Validate configuration (reject zero / inverted budgets).
    pub fn validate(&self) -> Result<()> {
        if self.max_model_loads == 0
            || self.max_local_stt == 0
            || self.max_local_tts == 0
            || self.max_remote == 0
            || self.max_blocking == 0
            || self.max_cpu_threads == 0
        {
            return Err(crate::error::UserError::InvalidConfig {
                reason: "governor permit and CPU budgets must be >= 1".into(),
            }
            .into());
        }
        Ok(())
    }
}

struct CounterPool {
    max: usize,
    in_use: AtomicUsize,
}

impl CounterPool {
    fn new(max: usize) -> Self {
        Self {
            max: max.max(1),
            in_use: AtomicUsize::new(0),
        }
    }

    fn try_acquire(&self) -> bool {
        loop {
            let cur = self.in_use.load(Ordering::SeqCst);
            if cur >= self.max {
                return false;
            }
            if self
                .in_use
                .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn release(&self) {
        let prev = self.in_use.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(prev > 0, "permit released more times than acquired");
    }

    fn in_use(&self) -> usize {
        self.in_use.load(Ordering::SeqCst)
    }

    fn max(&self) -> usize {
        self.max
    }
}

/// Process/engine resource governor.
pub struct ResourceGovernor {
    config: GovernorConfig,
    model_loads: CounterPool,
    local_stt: CounterPool,
    local_tts: CounterPool,
    remote: CounterPool,
    blocking: CounterPool,
    /// Threads currently allocated to STT jobs.
    cpu_threads_in_use: AtomicUsize,
    memory_reserved: AtomicU64,
    /// Serialize multi-permit acquisition to avoid deadlock across kinds.
    acquire_lock: Mutex<()>,
    /// Mutex paired with [`Self::waiters`] for permit queue parking (JOE-1831).
    wait_mutex: Mutex<()>,
    /// Signalled on every permit/memory/CPU release so waiters stop spinning.
    waiters: Condvar,
}

impl Default for ResourceGovernor {
    fn default() -> Self {
        Self::new(GovernorConfig::default())
    }
}

impl ResourceGovernor {
    pub fn new(config: GovernorConfig) -> Self {
        Self {
            model_loads: CounterPool::new(config.max_model_loads),
            local_stt: CounterPool::new(config.max_local_stt),
            local_tts: CounterPool::new(config.max_local_tts),
            remote: CounterPool::new(config.max_remote),
            blocking: CounterPool::new(config.max_blocking),
            cpu_threads_in_use: AtomicUsize::new(0),
            memory_reserved: AtomicU64::new(0),
            acquire_lock: Mutex::new(()),
            wait_mutex: Mutex::new(()),
            waiters: Condvar::new(),
            config,
        }
    }

    /// Wake all permit waiters (called after any resource release).
    fn notify_waiters(&self) {
        self.waiters.notify_all();
    }

    /// Park until a release, cancel, or the wait budget expires (JOE-1831).
    fn park_wait(&self, ctx: Option<&OpContext>, deadline: Instant) -> Result<()> {
        if self.config.fail_fast {
            return Ok(());
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(());
        }
        let rem = deadline.saturating_duration_since(now).min(WAIT_SLICE);
        // Prefer OpContext remaining so an absolute deadline is honoured promptly.
        let slice = ctx
            .and_then(|c| c.remaining())
            .map(|r| r.min(rem))
            .unwrap_or(rem);
        if slice.is_zero() {
            return Ok(());
        }
        let guard = self.wait_mutex.lock().unwrap_or_else(|e| e.into_inner());
        let (_g, _timeout) = self
            .waiters
            .wait_timeout(guard, slice)
            .unwrap_or_else(|e| e.into_inner());
        if let Some(c) = ctx {
            c.check()?;
        }
        Ok(())
    }

    pub fn config(&self) -> &GovernorConfig {
        &self.config
    }

    /// Process-wide default governor (desktop profile).
    ///
    /// Isolated engines can construct their own [`ResourceGovernor`] instead.
    pub fn process_global() -> Arc<Self> {
        use once_cell::sync::Lazy;
        static G: Lazy<Arc<ResourceGovernor>> = Lazy::new(|| Arc::new(ResourceGovernor::default()));
        Arc::clone(&G)
    }

    fn pool(&self, kind: PermitKind) -> &CounterPool {
        match kind {
            PermitKind::ModelLoad => &self.model_loads,
            PermitKind::LocalStt => &self.local_stt,
            PermitKind::LocalTts => &self.local_tts,
            PermitKind::Remote => &self.remote,
            PermitKind::Blocking => &self.blocking,
        }
    }

    /// Acquire a single permit, optionally waiting with cancel/deadline.
    pub fn acquire(
        self: &Arc<Self>,
        kind: PermitKind,
        ctx: Option<&OpContext>,
    ) -> Result<ResourcePermit> {
        let timeout = self.wait_budget(ctx);
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(c) = ctx {
                c.check()?;
            }
            if self.pool(kind).try_acquire() {
                return Ok(ResourcePermit {
                    governor: Arc::clone(self),
                    kind,
                    cpu_threads: 0,
                    memory: 0,
                    holds_blocking: false,
                    released: false,
                });
            }
            if self.config.fail_fast || Instant::now() >= deadline {
                return Err(ProviderError::Overload {
                    reason: format!(
                        "{kind:?} permits exhausted ({}/{})",
                        self.pool(kind).in_use(),
                        self.pool(kind).max()
                    ),
                }
                .into());
            }
            self.park_wait(ctx, deadline)?;
        }
    }

    fn wait_budget(&self, ctx: Option<&OpContext>) -> Duration {
        if self.config.fail_fast {
            return Duration::ZERO;
        }
        ctx.and_then(|c| c.remaining())
            .unwrap_or(self.config.queue_timeout)
            .min(self.config.queue_timeout)
    }

    /// Acquire STT permit + blocking permit + CPU thread budget (+ optional memory).
    ///
    /// Acquisition order is fixed (memory → STT → blocking → CPU) under a brief
    /// mutex so concurrent multi-resource acquires cannot deadlock. The mutex is
    /// not held across sleep/wait.
    pub fn acquire_stt(
        self: &Arc<Self>,
        cpu_threads: usize,
        memory: u64,
        ctx: Option<&OpContext>,
    ) -> Result<ResourcePermit> {
        let timeout = self.wait_budget(ctx);
        let deadline = Instant::now() + timeout;
        let want = cpu_threads.max(1);

        loop {
            if let Some(c) = ctx {
                c.check()?;
            }

            {
                let _order = self.acquire_lock.lock().unwrap_or_else(|e| e.into_inner());

                // Try memory first.
                if memory == 0 || self.try_reserve_memory(memory).is_ok() {
                    if self.local_stt.try_acquire() {
                        if self.blocking.try_acquire() {
                            if self.try_reserve_cpu(want) {
                                return Ok(ResourcePermit {
                                    governor: Arc::clone(self),
                                    kind: PermitKind::LocalStt,
                                    cpu_threads: want,
                                    memory,
                                    holds_blocking: true,
                                    released: false,
                                });
                            }
                            self.blocking.release();
                        }
                        self.local_stt.release();
                    }
                    self.release_memory(memory);
                }
            }

            if self.config.fail_fast || Instant::now() >= deadline {
                return Err(ProviderError::Overload {
                    reason: format!(
                        "STT resources unavailable (cpu want {want}, budget {})",
                        self.config.max_cpu_threads
                    ),
                }
                .into());
            }
            self.park_wait(ctx, deadline)?;
        }
    }

    /// Acquire local TTS + blocking permits (+ optional memory).
    pub fn acquire_tts(
        self: &Arc<Self>,
        memory: u64,
        ctx: Option<&OpContext>,
    ) -> Result<ResourcePermit> {
        let timeout = self.wait_budget(ctx);
        let deadline = Instant::now() + timeout;

        loop {
            if let Some(c) = ctx {
                c.check()?;
            }

            {
                let _order = self.acquire_lock.lock().unwrap_or_else(|e| e.into_inner());

                if memory == 0 || self.try_reserve_memory(memory).is_ok() {
                    if self.local_tts.try_acquire() {
                        if self.blocking.try_acquire() {
                            return Ok(ResourcePermit {
                                governor: Arc::clone(self),
                                kind: PermitKind::LocalTts,
                                cpu_threads: 0,
                                memory,
                                holds_blocking: true,
                                released: false,
                            });
                        }
                        self.local_tts.release();
                    }
                    self.release_memory(memory);
                }
            }

            if self.config.fail_fast || Instant::now() >= deadline {
                return Err(ProviderError::Overload {
                    reason: format!(
                        "LocalTts permits exhausted ({}/{})",
                        self.local_tts.in_use(),
                        self.local_tts.max()
                    ),
                }
                .into());
            }
            self.park_wait(ctx, deadline)?;
        }
    }

    fn try_reserve_memory(&self, bytes: u64) -> Result<()> {
        if bytes == 0 {
            return Ok(());
        }
        loop {
            let cur = self.memory_reserved.load(Ordering::SeqCst);
            let new = cur.saturating_add(bytes);
            if new > self.config.max_memory_bytes {
                return Err(ProviderError::Overload {
                    reason: format!(
                        "memory reservation {bytes} would exceed budget {} (in use {cur})",
                        self.config.max_memory_bytes
                    ),
                }
                .into());
            }
            if self
                .memory_reserved
                .compare_exchange(cur, new, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok(());
            }
        }
    }

    fn release_memory(&self, bytes: u64) {
        if bytes > 0 {
            self.memory_reserved.fetch_sub(bytes, Ordering::SeqCst);
            self.notify_waiters();
        }
    }

    fn try_reserve_cpu(&self, n: usize) -> bool {
        loop {
            let cur = self.cpu_threads_in_use.load(Ordering::SeqCst);
            if cur + n > self.config.max_cpu_threads {
                return false;
            }
            if self
                .cpu_threads_in_use
                .compare_exchange(cur, cur + n, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn release_cpu(&self, n: usize) {
        if n > 0 {
            self.cpu_threads_in_use.fetch_sub(n, Ordering::SeqCst);
            self.notify_waiters();
        }
    }

    /// How many Whisper threads a new STT job should use given remaining budget.
    pub fn recommend_stt_threads(&self) -> usize {
        let used = self.cpu_threads_in_use.load(Ordering::SeqCst);
        let rem = self.config.max_cpu_threads.saturating_sub(used).max(1);
        // Fair share: leave room for concurrent jobs when budget allows.
        let fair = (self.config.max_cpu_threads / self.config.max_local_stt.max(1)).max(1);
        fair.min(rem).clamp(1, 8)
    }

    pub fn stats(&self) -> GovernorStats {
        GovernorStats {
            model_loads: self.model_loads.in_use(),
            local_stt: self.local_stt.in_use(),
            local_tts: self.local_tts.in_use(),
            remote: self.remote.in_use(),
            blocking: self.blocking.in_use(),
            cpu_threads: self.cpu_threads_in_use.load(Ordering::SeqCst),
            memory_reserved: self.memory_reserved.load(Ordering::SeqCst),
            max_cpu_threads: self.config.max_cpu_threads,
            max_memory_bytes: self.config.max_memory_bytes,
        }
    }
}

/// Snapshot of governor occupancy.
#[derive(Debug, Clone)]
pub struct GovernorStats {
    pub model_loads: usize,
    pub local_stt: usize,
    pub local_tts: usize,
    pub remote: usize,
    pub blocking: usize,
    pub cpu_threads: usize,
    pub memory_reserved: u64,
    pub max_cpu_threads: usize,
    pub max_memory_bytes: u64,
}

/// RAII permit — releases all held resources on drop.
///
/// Holds an [`Arc`] so permits can cross `.await` points.
pub struct ResourcePermit {
    governor: Arc<ResourceGovernor>,
    kind: PermitKind,
    cpu_threads: usize,
    memory: u64,
    holds_blocking: bool,
    released: bool,
}

impl std::fmt::Debug for ResourcePermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourcePermit")
            .field("kind", &self.kind)
            .field("cpu_threads", &self.cpu_threads)
            .field("memory", &self.memory)
            .field("holds_blocking", &self.holds_blocking)
            .finish()
    }
}

impl ResourcePermit {
    pub fn kind(&self) -> PermitKind {
        self.kind
    }

    pub fn cpu_threads(&self) -> usize {
        self.cpu_threads
    }

    fn release_inner(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        if self.cpu_threads > 0 {
            self.governor.release_cpu(self.cpu_threads);
            self.cpu_threads = 0;
        }
        if self.holds_blocking {
            self.governor.blocking.release();
            self.holds_blocking = false;
        }
        if self.memory > 0 {
            self.governor.release_memory(self.memory);
            self.memory = 0;
        }
        self.governor.pool(self.kind).release();
        // Always wake waiters once per full permit release (CPU/memory already
        // notified when non-zero; a second notify_all is cheap and covers the
        // pure-permit case where only the kind counter changed).
        self.governor.notify_waiters();
    }
}

impl Drop for ResourcePermit {
    fn drop(&mut self) {
        self.release_inner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_cap() {
        let g = Arc::new(ResourceGovernor::new(GovernorConfig {
            max_local_stt: 1,
            fail_fast: true,
            ..GovernorConfig::default()
        }));
        let a = g.acquire(PermitKind::LocalStt, None).unwrap();
        let err = g.acquire(PermitKind::LocalStt, None).unwrap_err();
        assert!(matches!(
            err,
            crate::error::TranscriptionError::Provider(ProviderError::Overload { .. })
        ));
        drop(a);
        let _b = g.acquire(PermitKind::LocalStt, None).unwrap();
    }

    #[test]
    fn memory_budget() {
        let g = Arc::new(ResourceGovernor::new(GovernorConfig {
            max_memory_bytes: 1000,
            fail_fast: true,
            ..GovernorConfig::default()
        }));
        assert!(g.try_reserve_memory(600).is_ok());
        assert!(g.try_reserve_memory(600).is_err());
        g.release_memory(600);
        assert!(g.try_reserve_memory(600).is_ok());
    }

    #[test]
    fn cpu_budget() {
        let g = Arc::new(ResourceGovernor::new(GovernorConfig {
            max_cpu_threads: 4,
            max_local_stt: 4,
            max_blocking: 4,
            fail_fast: true,
            ..GovernorConfig::default()
        }));
        let p = g.acquire_stt(3, 0, None).unwrap();
        assert_eq!(p.cpu_threads(), 3);
        let err = g.acquire_stt(3, 0, None).unwrap_err();
        assert!(err.to_string().contains("CPU") || err.to_string().contains("overload"));
        drop(p);
        let _p2 = g.acquire_stt(2, 0, None).unwrap();
    }

    #[test]
    fn tts_releases_blocking() {
        let g = Arc::new(ResourceGovernor::new(GovernorConfig {
            max_local_tts: 1,
            max_blocking: 1,
            fail_fast: true,
            ..GovernorConfig::default()
        }));
        let p = g.acquire_tts(0, None).unwrap();
        let err = g.acquire_tts(0, None).unwrap_err();
        assert!(matches!(
            err,
            crate::error::TranscriptionError::Provider(ProviderError::Overload { .. })
        ));
        drop(p);
        let _p2 = g.acquire_tts(0, None).unwrap();
    }

    #[test]
    fn cancel_aborts_queue_wait() {
        let g = Arc::new(ResourceGovernor::new(GovernorConfig {
            max_local_stt: 1,
            fail_fast: false,
            queue_timeout: Duration::from_secs(5),
            ..GovernorConfig::default()
        }));
        let _hold = g.acquire(PermitKind::LocalStt, None).unwrap();
        let ctx = OpContext::new();
        ctx.cancel.cancel();
        let err = g.acquire(PermitKind::LocalStt, Some(&ctx)).unwrap_err();
        assert!(matches!(
            err,
            crate::error::TranscriptionError::Provider(ProviderError::Cancelled)
        ));
    }

    #[test]
    fn config_validate_rejects_zero() {
        let c = GovernorConfig {
            max_cpu_threads: 0,
            ..GovernorConfig::default()
        };
        assert!(c.validate().is_err());
    }
}
