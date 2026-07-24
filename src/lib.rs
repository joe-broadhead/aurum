//! # Aurum
//!
//! Local-first transcription library and CLI foundation.
//!
//! Aurum (Latin: *gold*) converts audio to text using local whisper.cpp models
//! by default, with an optional OpenRouter remote provider.
//!
//! The core is structured so it can later be published as a standalone crate
//! with minimal changes. The library API is **experimental** in v0.0.0 —
//! expect breaking changes.
//!
//! ## Example
//!
//! ```rust,no_run
//! use aurum::audio::load_audio;
//! use aurum::providers::{LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider};
//! use std::path::PathBuf;
//!
//! # async fn demo() -> aurum::error::Result<()> {
//! let audio = load_audio(std::path::Path::new("meeting.m4a")).await?;
//! let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"));
//! let result = provider
//!     .transcribe(
//!         &audio,
//!         &TranscriptionOptions {
//!             model: "base".into(),
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
pub mod cli;
pub mod config;
pub mod error;
pub mod model;
pub mod output;
pub mod postprocess;
pub mod providers;

pub use audio::{load_audio, AudioInput};
pub use config::Config;
pub use error::{Result, TranscriptionError};
pub use output::{format_result, OutputFormat};
pub use providers::{
    LocalWhisperProvider, OpenRouterProvider, Segment, TranscriptionOptions, TranscriptionProvider,
    TranscriptionResult,
};
