//! Transcription provider abstraction.

pub mod local;
pub mod openrouter;

use crate::audio::AudioInput;
use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Options controlling a single transcription request.
#[derive(Debug, Clone)]
pub struct TranscriptionOptions {
    /// Model name (local ggml name or remote model id).
    pub model: String,
    /// BCP-47 / ISO language code, or `"auto"`.
    pub language: String,
    /// Request segment-level timestamps when the provider supports them.
    pub timestamps: bool,
}

impl Default for TranscriptionOptions {
    fn default() -> Self {
        Self {
            model: crate::config::DEFAULT_LOCAL_MODEL.to_string(),
            language: crate::config::DEFAULT_LANGUAGE.to_string(),
            timestamps: false,
        }
    }
}

/// A single timed segment of transcript text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    pub text: String,
}

/// Normalized result returned by every provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub segments: Vec<Segment>,
    pub language: Option<String>,
    pub model: String,
    pub provider: String,
    pub duration_secs: f64,
}

/// Provider trait — the foundation for local and remote backends.
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Human-readable provider name (e.g. `"local"`, `"openrouter"`).
    fn name(&self) -> &'static str;

    /// Transcribe audio according to `options`.
    async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult>;
}

pub use local::LocalWhisperProvider;
pub use openrouter::OpenRouterProvider;
