//! # aurum-ffi
//!
//! **C ABI façade** over [`aurum_core`] for native embedders (ABI v2 / JOE-1577).
//!
//! On-device by default: PCM in → local whisper → optional rules cleanup / TTS.
//! Partials, mic capture, and UX policy stay in the host.
//!
//! ## Ownership & threading
//!
//! * Each [`Engine`] owns cache policy, exclusive blocking ops, metrics, and jobs.
//! * Prefer **jobs** (`start_*_job` / C `aurum_job_*`) so hosts never nest Tokio.
//! * Blocking exports remain for simple hosts; do not call them from inside a
//!   host async runtime task.
//! * `aurum_engine_shutdown` drains one engine; `aurum_shutdown_ex` is process-wide.
//!
//! ## Rust façade
//!
//! ```rust,no_run
//! use aurum_ffi::{CleanupStyle, Engine, EngineConfig, JobState, TranscribeOpts};
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), aurum_ffi::FfiError> {
//! let engine = Engine::new(EngineConfig {
//!     cache_dir: "/tmp/aurum-cache".into(),
//!     local_only: true,
//!     progress_logging: false,
//! })?;
//! // Nonblocking job path (safe from event-loop threads):
//! let job = engine.start_cleanup_job("um, hello", CleanupStyle::Clean)?;
//! let _ = job.wait(Some(Duration::from_secs(2)))?;
//! assert_eq!(job.state(), JobState::Completed);
//! engine.shutdown_engine(Duration::from_secs(2))?;
//! # Ok(())
//! # }
//! ```
//!
//! ## C ABI
//!
//! See `include/aurum.h` and the `extern "C"` functions in [`c_api`].

#![deny(unsafe_op_in_unsafe_fn)]

mod c_api;
mod error;
mod facade;
mod jobs;
mod runtime;
mod types;

pub use c_api::*;
pub use error::{FfiError, FfiStatus};
pub use facade::{cleanup_rules, shutdown, shutdown_with_timeout, Engine};
pub use jobs::{AbiCapabilities, Job, JobKind, JobResult, JobState};
pub use types::{
    CleanupStyle, EngineConfig, Segment, TranscribeOpts, Transcript, AURUM_ABI_MIN_VERSION,
    AURUM_ABI_VERSION, AURUM_SAMPLE_RATE,
};

/// Crate version string (SemVer from Cargo).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
