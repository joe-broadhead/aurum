//! Process-wide Tokio runtime for blocking FFI entry points.
//!
//! Concurrent `block_on` from different host threads is supported (multi-thread
//! runtime + `Handle::block_on`). Per-engine serialization is enforced by
//! [`crate::facade::Engine`]'s busy guard, not by locking this runtime.

use crate::error::FfiError;
use once_cell::sync::OnceCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::runtime::Runtime;

static RUNTIME: OnceCell<Runtime> = OnceCell::new();
static SHUT_DOWN: AtomicBool = AtomicBool::new(false);

/// Count of in-flight façade ops (preload / transcribe) across all engines.
static ACTIVE_OPS: AtomicUsize = AtomicUsize::new(0);

fn runtime() -> Result<&'static Runtime, FfiError> {
    if SHUT_DOWN.load(Ordering::SeqCst) {
        return Err(FfiError::state("aurum-ffi runtime has been shut down"));
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

pub fn begin_op() {
    ACTIVE_OPS.fetch_add(1, Ordering::SeqCst);
}

pub fn end_op() {
    ACTIVE_OPS.fetch_sub(1, Ordering::SeqCst);
}

/// Reject new work and wait briefly for in-flight ops, then mark shut down.
///
/// The Tokio `Runtime` remains until process exit (OnceCell); new calls fail with `STATE`.
pub fn shutdown_runtime() {
    // Stop accepting new ops first.
    SHUT_DOWN.store(true, Ordering::SeqCst);
    // Wait up to ~2s for in-flight work so whisper contexts are not torn down mid-decode.
    for _ in 0..200 {
        if ACTIVE_OPS.load(Ordering::SeqCst) == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
