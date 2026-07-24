//! # aurum-core
//!
//! Reusable local-first transcription library (experimental API).
//!
//! Aurum (Latin: *gold*) converts audio to text using local whisper.cpp models
//! by default, with an optional OpenRouter remote provider.
//!
//! The API may change without notice until a stable `0.1.0`.
//!
//! ## Example
//!
//! ```rust,no_run
//! use aurum_core::audio::load_audio;
//! use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider};
//! use std::path::PathBuf;
//!
//! # async fn demo() -> aurum_core::error::Result<()> {
//! let audio = load_audio(std::path::Path::new("meeting.m4a")).await?;
//! let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"));
//! let result = provider
//!     .transcribe(
//!         &audio,
//!         &TranscriptionOptions {
//!             model: "tiny-q5_1".into(),
//!             language: "auto".into(),
//!             timestamps: true,
//!         },
//!     )
//!     .await?;
//! println!("{}", result.text);
//! # Ok(())
//! # }
//! ```

pub mod audio;
pub mod config;
pub mod error;
pub mod model;
pub mod output;
pub mod postprocess;
pub mod providers;

pub use audio::{load_audio, AudioInput};
pub use config::Config;
pub use error::{Result, TranscriptionError};
pub use model::{list_models, ModelInfo, ModelStatus};
pub use output::{format_result, OutputFormat};
pub use providers::{
    LocalWhisperProvider, OpenRouterProvider, Segment, TranscriptionOptions, TranscriptionProvider,
    TranscriptionResult,
};
