//! Process-wide Tokio runtime for blocking FFI entry points.

use once_cell::sync::OnceCell;
use std::sync::Mutex;
use tokio::runtime::Runtime;

static RUNTIME: OnceCell<Mutex<Option<Runtime>>> = OnceCell::new();

fn cell() -> &'static Mutex<Option<Runtime>> {
    RUNTIME.get_or_init(|| {
        let rt = Runtime::new().unwrap_or_else(|e| {
            panic!("aurum-ffi: failed to create Tokio runtime: {e}");
        });
        Mutex::new(Some(rt))
    })
}

/// Run an async façade call on the shared runtime.
pub fn block_on<F, T>(fut: F) -> Result<T, crate::error::FfiError>
where
    F: std::future::Future<Output = T>,
{
    let guard = cell()
        .lock()
        .map_err(|_| crate::error::FfiError::internal("runtime mutex poisoned"))?;
    let rt = guard
        .as_ref()
        .ok_or_else(|| crate::error::FfiError::state("aurum-ffi runtime has been shut down"))?;
    Ok(rt.block_on(fut))
}

/// Drop the runtime (for clean process exit / tests). Idempotent.
pub fn shutdown_runtime() {
    if let Some(m) = RUNTIME.get() {
        if let Ok(mut g) = m.lock() {
            *g = None;
        }
    }
}
