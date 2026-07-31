//! Synchronized lifecycle: Running → ShuttingDown → Stopped (JOE-1594).

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const STATE_RUNNING: u8 = 0;
const STATE_SHUTTING_DOWN: u8 = 1;
const STATE_STOPPED: u8 = 2;

/// High-level lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Running,
    ShuttingDown,
    Stopped,
}

/// Shutdown outcome when active work does not drain in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownError {
    /// Timed out with N operations still active; contexts were NOT cleared.
    Busy { active: usize },
}

/// Process/engine lifecycle with atomic admission and active-op accounting.
///
/// Admission and op registration happen in one synchronized path so shutdown
/// cannot race a half-admitted operation.
#[derive(Debug)]
pub struct Lifecycle {
    state: AtomicU8,
    active: AtomicUsize,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(STATE_RUNNING),
            active: AtomicUsize::new(0),
        }
    }

    pub fn state(&self) -> LifecycleState {
        match self.state.load(Ordering::SeqCst) {
            STATE_RUNNING => LifecycleState::Running,
            STATE_SHUTTING_DOWN => LifecycleState::ShuttingDown,
            _ => LifecycleState::Stopped,
        }
    }

    pub fn active_ops(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// Atomically admit one operation if still Running.
    ///
    /// On success, active count is incremented; caller MUST drop the returned
    /// [`OpAdmission`] (or call [`Self::end_op`]) when the op finishes.
    pub fn try_begin_op(&self) -> Result<OpAdmission<'_>, AdmissionError> {
        // Fast path: reject if not running.
        let s = self.state.load(Ordering::SeqCst);
        if s != STATE_RUNNING {
            return Err(AdmissionError::NotRunning {
                state: decode_state(s),
            });
        }
        // Increment first, then re-check state to close the race with shutdown.
        self.active.fetch_add(1, Ordering::SeqCst);
        let s2 = self.state.load(Ordering::SeqCst);
        if s2 != STATE_RUNNING {
            self.active.fetch_sub(1, Ordering::SeqCst);
            return Err(AdmissionError::NotRunning {
                state: decode_state(s2),
            });
        }
        Ok(OpAdmission { life: self })
    }

    pub(crate) fn end_op(&self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }

    /// Close admission and wait for active ops to reach zero.
    ///
    /// On success transitions to Stopped. On timeout leaves ShuttingDown and
    /// returns [`ShutdownError::Busy`] — does **not** claim completion and does
    /// **not** authorize cache clears.
    ///
    /// Idempotent: calling while already Stopped succeeds. Calling while
    /// ShuttingDown continues waiting for the remaining active count.
    pub fn shutdown(&self, timeout: Duration) -> Result<(), ShutdownError> {
        let prev = self.state.load(Ordering::SeqCst);
        if prev == STATE_STOPPED {
            return Ok(());
        }
        // Running or ShuttingDown → ensure ShuttingDown.
        self.state
            .compare_exchange(
                STATE_RUNNING,
                STATE_SHUTTING_DOWN,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .ok();
        // If another thread already stopped us, accept.
        if self.state.load(Ordering::SeqCst) == STATE_STOPPED {
            return Ok(());
        }
        self.state.store(STATE_SHUTTING_DOWN, Ordering::SeqCst);

        let deadline = Instant::now() + timeout;
        while self.active.load(Ordering::SeqCst) > 0 {
            if Instant::now() >= deadline {
                let active = self.active.load(Ordering::SeqCst);
                return Err(ShutdownError::Busy { active });
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.state.store(STATE_STOPPED, Ordering::SeqCst);
        Ok(())
    }

    /// Force Stopped only when active is zero (test helper / finalizer).
    pub fn mark_stopped_if_idle(&self) -> bool {
        if self.active.load(Ordering::SeqCst) == 0 {
            self.state.store(STATE_STOPPED, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    /// Whether new work may be admitted.
    pub fn is_running(&self) -> bool {
        self.state.load(Ordering::SeqCst) == STATE_RUNNING
    }
}

fn decode_state(s: u8) -> LifecycleState {
    match s {
        STATE_RUNNING => LifecycleState::Running,
        STATE_SHUTTING_DOWN => LifecycleState::ShuttingDown,
        _ => LifecycleState::Stopped,
    }
}

/// Error when admission is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    NotRunning { state: LifecycleState },
}

impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRunning { state } => {
                write!(f, "runtime is not accepting work (state={state:?})")
            }
        }
    }
}

impl std::error::Error for AdmissionError {}

/// RAII admission ticket — drops unregister the op (panic-safe).
pub struct OpAdmission<'a> {
    life: &'a Lifecycle,
}

impl Drop for OpAdmission<'_> {
    fn drop(&mut self) {
        self.life.end_op();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn admit_and_release() {
        let life = Lifecycle::new();
        let g = life.try_begin_op().unwrap();
        assert_eq!(life.active_ops(), 1);
        drop(g);
        assert_eq!(life.active_ops(), 0);
    }

    #[test]
    fn shutdown_blocks_admission() {
        let life = Lifecycle::new();
        life.shutdown(Duration::from_millis(100)).unwrap();
        assert!(matches!(
            life.try_begin_op(),
            Err(AdmissionError::NotRunning { .. })
        ));
        assert_eq!(life.state(), LifecycleState::Stopped);
    }

    #[test]
    fn shutdown_waits_for_active() {
        let life = Arc::new(Lifecycle::new());
        let g = life.try_begin_op().unwrap();
        let life2 = Arc::clone(&life);
        let h = thread::spawn(move || life2.shutdown(Duration::from_secs(2)));
        thread::sleep(Duration::from_millis(50));
        assert_eq!(life.state(), LifecycleState::ShuttingDown);
        drop(g);
        h.join().unwrap().unwrap();
        assert_eq!(life.state(), LifecycleState::Stopped);
    }

    #[test]
    fn shutdown_timeout_busy() {
        let life = Lifecycle::new();
        let _g = life.try_begin_op().unwrap();
        let err = life.shutdown(Duration::from_millis(30)).unwrap_err();
        assert!(matches!(err, ShutdownError::Busy { active: 1 }));
        assert_eq!(life.state(), LifecycleState::ShuttingDown);
    }

    #[test]
    fn shutdown_idempotent_when_stopped() {
        let life = Lifecycle::new();
        life.shutdown(Duration::from_millis(100)).unwrap();
        life.shutdown(Duration::from_millis(100)).unwrap();
        assert_eq!(life.state(), LifecycleState::Stopped);
    }

    #[test]
    fn race_admission_with_shutdown() {
        let life = Arc::new(Lifecycle::new());
        let mut handles = vec![];
        for _ in 0..8 {
            let l = Arc::clone(&life);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    if let Ok(g) = l.try_begin_op() {
                        thread::sleep(Duration::from_micros(50));
                        drop(g);
                    }
                }
            }));
        }
        thread::sleep(Duration::from_millis(5));
        let _ = life.shutdown(Duration::from_secs(2));
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(life.active_ops(), 0);
    }
}
