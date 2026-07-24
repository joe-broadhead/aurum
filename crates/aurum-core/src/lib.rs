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
//! use aurum_core::audio::{AudioInput, WHISPER_SAMPLE_RATE};
//! use aurum_core::pcm::PcmBuffer;
//! use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};
//! use std::path::PathBuf;
//!
//! # async fn demo() -> aurum_core::error::Result<()> {
//! let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"))
//!     .with_progress(false)
//!     .with_local_only(false);
//! provider.preload("tiny-q5_1").await?;
//!
//! // Mic host: push PCM, then finalize (no files / ffmpeg).
//! let mut buf = PcmBuffer::dictation();
//! buf.push(&[0.0f32; 1600])?;
//! let result = provider
//!     .transcribe_pcm(
//!         buf.samples(),
//!         &TranscriptionOptions {
//!             model: "tiny-q5_1".into(),
//!             language: "en".into(),
//!             timestamps: false,
//!         },
//!     )
//!     .await?;
//! let _ = AudioInput::from_pcm_slice(buf.samples(), WHISPER_SAMPLE_RATE)?;
//! println!("{}", result.text);
//! aurum_core::providers::local::clear_context_cache();
//! # Ok(())
//! # }
//! ```

pub mod audio;
pub mod config;
pub mod error;
pub mod model;
pub mod output;
pub mod pcm;
pub mod postprocess;
pub mod providers;

pub use audio::{load_audio, AudioInput, WHISPER_SAMPLE_RATE};
pub use config::Config;
pub use error::{Result, TranscriptionError};
pub use model::{list_models, DownloadProgress, EnsureModelOptions, ModelInfo, ModelStatus};
pub use output::{format_result, OutputFormat};
pub use pcm::PcmBuffer;
pub use providers::{
    LocalWhisperProvider, OpenRouterProvider, Segment, TranscriptionOptions, TranscriptionProvider,
    TranscriptionResult,
};
