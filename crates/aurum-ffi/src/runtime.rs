//! Process-wide Tokio runtime and synchronized lifecycle (JOE-1594).
//!
//! Concurrent `block_on` from different host threads is supported (multi-thread
//! runtime + `Handle::block_on`). Per-engine serialization is enforced by
//! [`crate::facade::Engine`]'s busy guard, not by locking this runtime.
//!
//! Admission and shutdown use a single [`Lifecycle`] state machine
//! (Running → ShuttingDown → Stopped) so new work cannot enter after shutdown
//! begins and cache clears only happen when active ops are proven zero.

use crate::error::FfiError;
use aurum_core::runtime::{Lifecycle, OpAdmission, ShutdownError};
use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use std::time::Duration;
use tokio::runtime::Runtime;

static RUNTIME: OnceCell<Runtime> = OnceCell::new();
static LIFECYCLE: Lazy<Lifecycle> = Lazy::new(Lifecycle::new);

fn runtime() -> Result<&'static Runtime, FfiError> {
    if !LIFECYCLE.is_running() {
        return Err(FfiError::state(format!(
            "aurum-ffi runtime is not accepting work (state={:?})",
            LIFECYCLE.state()
        )));
    }
    Ok(RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("aurum-ffi")
            .build()
            .unwrap_or_else(|e| panic!("aurum-ffi: failed to create Tokio runtime: {e}"))
    }))
}

/// Run an async façade call on the shared runtime (does not hold a process-wide mutex).
pub fn block_on<F, T>(fut: F) -> Result<T, FfiError>
where
    F: std::future::Future<Output = T>,
{
    let rt = runtime()?;
    // Handle::block_on allows concurrent callers on a multi-thread runtime.
    Ok(rt.handle().block_on(fut))
}

/// Tokio handle for nonblocking job spawn (JOE-1623).
///
/// Hosts should prefer [`crate::jobs`] over nested `block_on` inside another
/// runtime; spawn + poll/wait does not require the caller to own a Tokio runtime.
pub fn handle() -> Result<tokio::runtime::Handle, FfiError> {
    Ok(runtime()?.handle().clone())
}

/// Whether the process lifecycle is still accepting work.
pub fn is_running() -> bool {
    LIFECYCLE.is_running()
}

/// Admit a long-lived job into the process lifecycle active count (JOE-1577).
///
/// Hold the ticket for the full job lifetime so `aurum_shutdown_ex` cannot
/// clear whisper caches while jobs still run.
pub fn begin_job() -> Result<OpAdmission<'static>, FfiError> {
    begin_op()
}

/// Atomically admit one op if the lifecycle is still Running.
///
/// Drop the returned ticket to unregister (panic-safe).
pub fn begin_op() -> Result<OpAdmission<'static>, FfiError> {
    LIFECYCLE
        .try_begin_op()
        .map_err(|e| FfiError::state(e.to_string()))
}

/// Result of process teardown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// Active count reached zero; safe to clear native contexts.
    Stopped,
    /// Timed out with work still active; caches must NOT be cleared.
    Busy { active: usize },
}

/// Close admission and wait for in-flight ops.
///
/// On [`ShutdownOutcome::Stopped`], callers may clear whisper contexts.
/// On [`ShutdownOutcome::Busy`], do **not** clear contexts — native work may
/// still be using them.
///
/// The Tokio `Runtime` remains until process exit (OnceCell); new calls fail
/// with `STATE` after shutdown begins.
pub fn shutdown_runtime(timeout: Duration) -> ShutdownOutcome {
    match LIFECYCLE.shutdown(timeout) {
        Ok(()) => ShutdownOutcome::Stopped,
        Err(ShutdownError::Busy { active }) => ShutdownOutcome::Busy { active },
    }
}

/// Default drain budget for the void `aurum_shutdown` wrapper.
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
