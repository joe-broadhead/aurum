//! Transcription provider abstraction.

pub mod local;
pub mod openrouter;

use crate::audio::AudioInput;
use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// How a backend produces transcripts — affects timestamp trust and UX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// Dedicated ASR (e.g. whisper.cpp). Timestamps are engine-derived.
    Asr,
    /// Multimodal LLM asked to transcribe. Text may paraphrase; timestamps are unreliable.
    LlmAssisted,
}

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
    /// Backend class — consumers should treat LLM timestamps as best-effort.
    #[serde(default = "default_backend_kind")]
    pub backend_kind: BackendKind,
    /// Whether segment timestamps are considered reliable.
    #[serde(default = "default_true")]
    pub timestamps_reliable: bool,
    /// Post-ASR cleanup style applied to [`Self::text`] (default: raw).
    #[serde(default)]
    pub cleanup_style: crate::cleanup::CleanupStyle,
    /// Cleanup backend used, if any cleanup beyond raw was applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_provider: Option<crate::cleanup::CleanupProviderKind>,
    /// Pre-cleanup ASR text when cleanup rewrote [`Self::text`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_text: Option<String>,
}

fn default_backend_kind() -> BackendKind {
    BackendKind::Asr
}
fn default_true() -> bool {
    true
}

impl TranscriptionResult {
    pub fn local(
        text: String,
        segments: Vec<Segment>,
        language: Option<String>,
        model: String,
        duration_secs: f64,
    ) -> Self {
        Self {
            text,
            segments,
            language,
            model,
            provider: "local".into(),
            duration_secs,
            backend_kind: BackendKind::Asr,
            timestamps_reliable: true,
            cleanup_style: crate::cleanup::CleanupStyle::Raw,
            cleanup_provider: None,
            original_text: None,
        }
    }

    pub fn openrouter(
        text: String,
        segments: Vec<Segment>,
        language: Option<String>,
        model: String,
        duration_secs: f64,
        _timestamps_requested: bool,
    ) -> Self {
        Self {
            text,
            segments,
            language,
            model,
            provider: "openrouter".into(),
            duration_secs,
            backend_kind: BackendKind::LlmAssisted,
            // LLM timestamps are never treated as reliable ASR timing.
            timestamps_reliable: false,
            cleanup_style: crate::cleanup::CleanupStyle::Raw,
            cleanup_provider: None,
            original_text: None,
        }
    }
}

/// Provider trait — the foundation for local and remote backends.
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Human-readable provider name (e.g. `"local"`, `"openrouter"`).
    fn name(&self) -> &'static str;

    /// Backend classification.
    fn backend_kind(&self) -> BackendKind;

    /// Whether this provider can emit trustworthy media timestamps.
    fn timestamps_reliable(&self) -> bool {
        matches!(self.backend_kind(), BackendKind::Asr)
    }

    /// Transcribe audio according to `options`.
    async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult>;
}

pub use local::LocalWhisperProvider;
pub use openrouter::OpenRouterProvider;
