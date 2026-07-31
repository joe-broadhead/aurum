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
    /// Optional cooperative cancel flag (honoured by local whisper decode).
    pub cancel: Option<crate::cancel::CancelFlag>,
}

impl Default for TranscriptionOptions {
    fn default() -> Self {
        Self {
            model: crate::config::DEFAULT_LOCAL_MODEL.to_string(),
            language: crate::config::DEFAULT_LANGUAGE.to_string(),
            timestamps: false,
            cancel: None,
        }
    }
}

impl TranscriptionOptions {
    pub fn with_cancel(mut self, flag: crate::cancel::CancelFlag) -> Self {
        self.cancel = Some(flag);
        self
    }
}

/// A single timed segment of transcript text.
///
/// Prefer [`Segment::try_new`] for validated construction. `Deserialize` is an
/// **untrusted DTO path** — call [`Segment::validate`] before trusting timings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    pub text: String,
}

impl Segment {
    /// Construct a segment, rejecting NaN/Inf, negatives, and inverted ranges (JOE-1781).
    pub fn try_new(start: f64, end: f64, text: impl Into<String>) -> Result<Self> {
        let s = Self {
            start,
            end,
            text: text.into(),
        };
        s.validate()?;
        Ok(s)
    }

    /// Validate timestamp finite-ness and ordering.
    pub fn validate(&self) -> Result<()> {
        if !self.start.is_finite() || !self.end.is_finite() {
            return Err(crate::error::UserError::Other {
                message: format!(
                    "segment timestamps must be finite (start={}, end={})",
                    self.start, self.end
                ),
            }
            .into());
        }
        if self.start < 0.0 || self.end < 0.0 {
            return Err(crate::error::UserError::Other {
                message: format!(
                    "segment timestamps must be non-negative (start={}, end={})",
                    self.start, self.end
                ),
            }
            .into());
        }
        if self.end < self.start {
            return Err(crate::error::UserError::Other {
                message: format!(
                    "segment end before start (start={}, end={})",
                    self.start, self.end
                ),
            }
            .into());
        }
        Ok(())
    }
}

/// Normalized result returned by every provider.
///
/// Prefer builders [`TranscriptionResult::local`] / [`TranscriptionResult::openrouter`].
/// `Deserialize` is untrusted — validate segments before relying on timings.
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
    /// Pre-cleanup ASR segments when cleanup rewrote or cleared timings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_segments: Option<Vec<Segment>>,
    /// Segment policy that was applied during cleanup (when not raw).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_segment_policy: Option<crate::cleanup::SegmentCleanupPolicy>,
}

fn default_backend_kind() -> BackendKind {
    BackendKind::Asr
}
fn default_true() -> bool {
    true
}

impl TranscriptionResult {
    /// Validate all segments (finite, ordered timestamps).
    pub fn validate_segments(&self) -> Result<()> {
        for (i, seg) in self.segments.iter().enumerate() {
            if let Err(e) = seg.validate() {
                return Err(crate::error::UserError::Other {
                    message: format!("segment[{i}]: {e}"),
                }
                .into());
            }
        }
        if !self.duration_secs.is_finite() || self.duration_secs < 0.0 {
            return Err(crate::error::UserError::Other {
                message: format!(
                    "duration_secs must be finite and non-negative (got {})",
                    self.duration_secs
                ),
            }
            .into());
        }
        Ok(())
    }

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
            original_segments: None,
            cleanup_segment_policy: None,
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
            original_segments: None,
            cleanup_segment_policy: None,
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
pub use openrouter::{OpenRouterProvider, OpenRouterSttMode, SttPath};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_try_new_accepts_valid() {
        let s = Segment::try_new(0.0, 1.5, "hello").unwrap();
        assert_eq!(s.start, 0.0);
        assert_eq!(s.end, 1.5);
        assert_eq!(s.text, "hello");
    }

    #[test]
    fn segment_try_new_rejects_nan() {
        assert!(Segment::try_new(f64::NAN, 1.0, "x").is_err());
        assert!(Segment::try_new(0.0, f64::INFINITY, "x").is_err());
    }

    #[test]
    fn segment_try_new_rejects_negative_and_inverted() {
        assert!(Segment::try_new(-0.1, 1.0, "x").is_err());
        assert!(Segment::try_new(2.0, 1.0, "x").is_err());
    }

    #[test]
    fn segment_validate_ok_on_zero_length() {
        // Zero-duration is allowed (start == end).
        Segment::try_new(1.0, 1.0, "").unwrap();
    }
}
