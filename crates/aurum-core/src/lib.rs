//! # aurum-core
//!
//! Reusable **on-device speech I/O** library (experimental API).
//!
//! - **STT** — local whisper.cpp by default; optional OpenRouter
//! - **TTS** — local ONNX KittenTTS (cargo feature `tts`, default on)
//! - **Cleanup** — rules or optional LLM post-edit
//!
//! Tagline: *Speech both ways. On-device by default.*
//!
//! The API may change without notice until a stable `0.1.0`.
//!
//! ## STT example
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
//! let mut buf = PcmBuffer::dictation();
//! buf.push(&[0.0f32; 1600])?;
//! let result = provider
//!     .transcribe_pcm(
//!         buf.samples(),
//!         &TranscriptionOptions {
//!             model: "tiny-q5_1".into(),
//!             language: "en".into(),
//!             timestamps: false,
//!             cancel: None,
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
pub mod cancel;
pub mod cleanup;
pub mod config;
pub mod error;
pub mod model;
pub mod output;
pub mod pcm;
pub mod postprocess;
pub mod providers;
#[cfg(feature = "tts")]
pub mod tts;
pub mod window;

pub use audio::{load_audio, AudioInput, WHISPER_SAMPLE_RATE};
pub use cancel::CancelFlag;
pub use cleanup::{
    apply_cleanup, apply_cleanup_with_segments, cleanup_text, CleanupProviderKind, CleanupReport,
    CleanupResult, CleanupStyle, OpenRouterCleanup, RulesCleanup, SegmentCleanupPolicy,
    TextCleanup,
};
pub use config::Config;
pub use error::{Result, TranscriptionError};
pub use model::{list_models, DownloadProgress, EnsureModelOptions, ModelInfo, ModelStatus};
pub use output::{
    commit_text, format_result, write_result_to_path, CommitMode, OutputFormat, OutputTransaction,
    SymlinkPolicy,
};
pub use pcm::PcmBuffer;
pub use providers::{
    LocalWhisperProvider, OpenRouterProvider, Segment, TranscriptionOptions, TranscriptionProvider,
    TranscriptionResult,
};
pub use window::{PartialClock, PartialWindowPolicy};

#[cfg(feature = "tts")]
pub use tts::{
    format_model_list as format_tts_model_list, format_voice_list as format_tts_voice_list,
    list_models as list_tts_models, list_voices as list_tts_voices, resolve_voice_for_model,
    write_wav_i16_mono_atomic, write_wav_i16_mono_transaction, BackendKind as TtsBackendKind,
    LocalTtsProvider, SynthesisOptions, SynthesisProvider, SynthesisResult, DEFAULT_TTS_MODEL,
    DEFAULT_TTS_VOICE,
};
