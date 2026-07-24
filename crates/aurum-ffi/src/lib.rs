//! # aurum-ffi
//!
//! Stable-ish **C ABI façade** over [`aurum_core`] for native embedders.
//!
//! On-device by default: PCM in → local whisper → optional rules cleanup.
//! Partials, mic capture, and UX policy stay in the host.
//!
//! ## Rust façade
//!
//! ```rust,no_run
//! use aurum_ffi::{CleanupStyle, Engine, EngineConfig, TranscribeOpts};
//!
//! # fn main() -> Result<(), aurum_ffi::FfiError> {
//! let engine = Engine::new(EngineConfig {
//!     cache_dir: "/tmp/aurum-cache".into(),
//!     local_only: true,
//!     progress_logging: false,
//! })?;
//! if engine.is_model_ready("tiny-q5_1") {
//!     engine.preload("tiny-q5_1")?;
//!     let t = engine.transcribe_pcm(
//!         &[0.0f32; 1600],
//!         &TranscribeOpts {
//!             model: "tiny-q5_1".into(),
//!             language: "en".into(),
//!             timestamps: false,
//!         },
//!     )?;
//!     let cleaned = aurum_ffi::cleanup_rules(&t.text, CleanupStyle::Clean)?;
//!     let _ = cleaned;
//! }
//! aurum_ffi::shutdown();
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
mod runtime;
mod types;

pub use c_api::*;
pub use error::{FfiError, FfiStatus};
pub use facade::{cleanup_rules, shutdown, Engine};
pub use types::{
    CleanupStyle, EngineConfig, Segment, TranscribeOpts, Transcript, AURUM_ABI_VERSION,
    AURUM_SAMPLE_RATE,
};

/// Crate version string (SemVer from Cargo).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
