//! Versioned external JSON DTOs (JOE-1614).
//!
//! In-memory domain types are converted *to* these DTOs for CLI/embed JSON.
//! Deserializing a DTO does **not** reconstruct native audio buffers.

use crate::cleanup::{CleanupProviderKind, CleanupStyle, SegmentCleanupPolicy};
use crate::error::TranscriptionError;
use crate::providers::{BackendKind, Segment, TranscriptionResult};
use serde::{Deserialize, Serialize};

/// Current STT result schema version for `--emit-json` / library export.
///
/// v2: segment `timestamp_source` provenance (JOE-2219). Absent field deserializes
/// as `unavailable`; legacy v1 payloads still parse.
pub const STT_RESULT_SCHEMA_VERSION: u32 = 2;
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
        let mut merged = result.warnings().to_vec();
        for w in warnings {
            if !merged.iter().any(|existing| existing == w) {
                merged.push(w.clone());
            }
        }
        Self {
            schema_version: STT_RESULT_SCHEMA_VERSION,
            text: result.text().to_string(),
            language: result.language().map(|s| s.to_string()),
            model: result.model().to_string(),
            provider: result.provider().to_string(),
            duration_secs: finite_or_zero(result.duration_secs()),
            backend_kind: result.backend_kind(),
            timestamps_reliable: result.timestamps_reliable(),
            cleanup_style: result.cleanup_style(),
            cleanup_provider: result.cleanup_provider(),
            original_text: result.original_text().map(|s| s.to_string()),
            original_segments: result.original_segments().map(|s| s.to_vec()),
            cleanup_segment_policy: result.cleanup_segment_policy(),
            segments: result
                .segments()
                .iter()
                .map(|s| {
                    Segment::from_parts_with_source(
                        finite_or_zero(s.start()),
                        finite_or_zero(s.end()),
                        s.text().to_string(),
                        s.timestamp_source(),
                    )
                })
                .collect(),
            normalization_warnings: merged,
            request_id: None,
        }
    }

    /// Convert this DTO into a validated domain [`TranscriptionResult`] (JOE-1809).
    pub fn into_domain(self) -> crate::error::Result<TranscriptionResult> {
        TranscriptionResult::try_from_dto(&self)
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
            backend_kind: r.backend_kind.as_str().into(),
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

    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    /// Pretty JSON that never emits NaN/Inf (finite-only fields).
    pub fn to_json_pretty(&self) -> Result<String, TranscriptionError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| TranscriptionError::internal(format!("ErrorDto serialize failed: {e}")))
    }

    /// Compact single-line JSON for machine consumers.
    pub fn to_json(&self) -> Result<String, TranscriptionError> {
        serde_json::to_string(self)
            .map_err(|e| TranscriptionError::internal(format!("ErrorDto serialize failed: {e}")))
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
            vec![Segment::from_parts_unchecked(0.0, 1.0, "hi".to_string())],
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
        assert_eq!(back.schema_version, STT_RESULT_SCHEMA_VERSION);
    }

    #[test]
    fn nan_duration_sanitized() {
        let mut r = TranscriptionResult::local("x".into(), vec![], None, "m".into(), f64::NAN);
        r.set_duration_secs(f64::NAN);
        let dto = SttResultDto::from_result(&r);
        assert_eq!(dto.duration_secs, 0.0);
    }

    #[test]
    fn error_dto_json_has_schema_and_exit_code() {
        let err = crate::error::UserError::MissingApiKey.into();
        let dto = ErrorDto::from_error(&err);
        let json = dto.to_json_pretty().unwrap();
        assert!(json.contains("schema_version"));
        assert!(json.contains("exit_code"));
        assert!(json.contains("category"));
        assert_eq!(dto.schema_version, ERROR_SCHEMA_VERSION);
        assert_eq!(dto.exit_code, err.exit_code());
    }

    #[cfg(feature = "tts")]
    #[test]
    fn tts_meta_local_and_remote_backend_kind() {
        use crate::tts::{BackendKind, SynthesisResult};

        let local = SynthesisResult {
            pcm_i16_mono: vec![1, 2, 3],
            sample_rate_hz: 24_000,
            channels: 1,
            backend_kind: BackendKind::Local,
            provider: "local".into(),
            model: "kitten-nano-int8".into(),
            voice: "Luna".into(),
            language: "en".into(),
            duration_ms: 1,
            text_chars: 1,
            text_truncated: false,
            chunk_count: 1,
            synthesized_chars: 1,
            adapter: None,
            trust: None,
            provenance: None,
        };
        let dto = TtsMetaDto::from_result(&local);
        assert_eq!(dto.schema_version, TTS_META_SCHEMA_VERSION);
        assert_eq!(dto.backend_kind, "local");
        // PCM never in DTO JSON.
        let json = serde_json::to_string(&dto).unwrap();
        assert!(!json.contains("pcm"));
        assert_eq!(dto.schema_version, 1);

        let mut remote = local.clone();
        remote.backend_kind = BackendKind::Remote;
        remote.provider = "openai".into();
        let dto_r = TtsMetaDto::from_result(&remote);
        assert_eq!(dto_r.backend_kind, "remote");
        // Adding Remote does not bump schema_version (backend_kind was already a string).
        assert_eq!(dto_r.schema_version, 1);
    }
}
