//! Versioned external JSON DTOs (JOE-1614).
//!
//! In-memory domain types are converted *to* these DTOs for CLI/embed JSON.
//! Deserializing a DTO does **not** reconstruct native audio buffers.

use crate::cleanup::{CleanupProviderKind, CleanupStyle, SegmentCleanupPolicy};
use crate::error::TranscriptionError;
use crate::providers::{BackendKind, Segment, TranscriptionResult};
use serde::{Deserialize, Serialize};

/// Current STT result schema version for `--emit-json` / library export.
pub const STT_RESULT_SCHEMA_VERSION: u32 = 1;
/// TTS metadata schema (PCM never included).
pub const TTS_META_SCHEMA_VERSION: u32 = 1;
/// Error envelope schema.
pub const ERROR_SCHEMA_VERSION: u32 = 1;

/// Versioned STT result DTO (public contract).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SttResultDto {
    pub schema_version: u32,
    pub text: String,
    pub language: Option<String>,
    pub model: String,
    pub provider: String,
    pub duration_secs: f64,
    pub backend_kind: BackendKind,
    pub timestamps_reliable: bool,
    pub cleanup_style: CleanupStyle,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_provider: Option<CleanupProviderKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_segments: Option<Vec<Segment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_segment_policy: Option<SegmentCleanupPolicy>,
    pub segments: Vec<Segment>,
    /// Normalization warnings (empty when none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub normalization_warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<u64>,
}

impl SttResultDto {
    pub fn from_result(result: &TranscriptionResult) -> Self {
        Self::from_result_with_warnings(result, &[])
    }

    pub fn from_result_with_warnings(result: &TranscriptionResult, warnings: &[String]) -> Self {
        Self {
            schema_version: STT_RESULT_SCHEMA_VERSION,
            text: result.text.clone(),
            language: result.language.clone(),
            model: result.model.clone(),
            provider: result.provider.clone(),
            duration_secs: finite_or_zero(result.duration_secs),
            backend_kind: result.backend_kind,
            timestamps_reliable: result.timestamps_reliable,
            cleanup_style: result.cleanup_style,
            cleanup_provider: result.cleanup_provider,
            original_text: result.original_text.clone(),
            original_segments: result.original_segments.clone(),
            cleanup_segment_policy: result.cleanup_segment_policy,
            segments: result
                .segments
                .iter()
                .map(|s| Segment {
                    start: finite_or_zero(s.start),
                    end: finite_or_zero(s.end),
                    text: s.text.clone(),
                })
                .collect(),
            normalization_warnings: warnings.to_vec(),
            request_id: None,
        }
    }

    /// Pretty JSON that never emits NaN/Inf.
    pub fn to_json_pretty(&self) -> Result<String, TranscriptionError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| TranscriptionError::internal(format!("STT DTO serialize failed: {e}")))
    }
}

/// TTS metadata DTO — **no PCM bytes**.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsMetaDto {
    pub schema_version: u32,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub backend_kind: String,
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub language: String,
    pub duration_ms: u64,
    pub text_chars: usize,
    pub text_truncated: bool,
    pub chunk_count: usize,
    pub synthesized_chars: usize,
    /// Adapter id (JOE-1576); omitted when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    /// Trust mode: builtin | verified | local_unverified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust: Option<String>,
    /// Provenance: builtin | custom | local_pack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

#[cfg(feature = "tts")]
impl TtsMetaDto {
    pub fn from_result(r: &crate::tts::SynthesisResult) -> Self {
        Self {
            schema_version: TTS_META_SCHEMA_VERSION,
            sample_rate_hz: r.sample_rate_hz,
            channels: r.channels,
            backend_kind: match r.backend_kind {
                crate::tts::BackendKind::Local => "local".into(),
            },
            provider: r.provider.clone(),
            model: r.model.clone(),
            voice: r.voice.clone(),
            language: r.language.clone(),
            duration_ms: r.duration_ms,
            text_chars: r.text_chars,
            text_truncated: r.text_truncated,
            chunk_count: r.chunk_count,
            synthesized_chars: r.synthesized_chars,
            adapter: r.adapter.clone(),
            trust: r.trust.clone(),
            provenance: r.provenance.clone(),
        }
    }
}

/// Stable error envelope for JSON/CLI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorDto {
    pub schema_version: u32,
    pub category: String,
    pub retryable: bool,
    pub message: String,
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
}

impl ErrorDto {
    pub fn from_error(err: &TranscriptionError) -> Self {
        Self {
            schema_version: ERROR_SCHEMA_VERSION,
            category: err.error_category().as_str().into(),
            retryable: err.retryable(),
            message: err.to_string(),
            exit_code: err.exit_code(),
            operation: None,
        }
    }
}

fn finite_or_zero(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stt_dto_roundtrip_fields() {
        let r = TranscriptionResult::local(
            "hi".into(),
            vec![Segment {
                start: 0.0,
                end: 1.0,
                text: "hi".into(),
            }],
            Some("en".into()),
            "base".into(),
            1.0,
        );
        let dto = SttResultDto::from_result(&r);
        let json = dto.to_json_pretty().unwrap();
        assert!(json.contains("schema_version"));
        assert!(!json.contains("NaN"));
        let back: SttResultDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, "hi");
        assert_eq!(back.schema_version, 1);
    }

    #[test]
    fn nan_duration_sanitized() {
        let mut r = TranscriptionResult::local("x".into(), vec![], None, "m".into(), f64::NAN);
        r.duration_secs = f64::NAN;
        let dto = SttResultDto::from_result(&r);
        assert_eq!(dto.duration_secs, 0.0);
    }
}
