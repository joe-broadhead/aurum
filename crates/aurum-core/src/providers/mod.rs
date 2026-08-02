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
/// Fields are **private** (JOE-1786). Construct with [`Segment::try_new`] (fail closed)
/// or deserialize then [`Segment::validate`]. Prefer accessors over free mutation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    /// Start time in seconds.
    start: f64,
    /// End time in seconds.
    end: f64,
    text: String,
    /// How timing was obtained (JOE-2219). Defaults to unavailable when absent.
    #[serde(default)]
    timestamp_source: crate::remote::TimestampSource,
}

impl Segment {
    /// Construct a segment, rejecting NaN/Inf, negatives, and inverted ranges (JOE-1781).
    pub fn try_new(start: f64, end: f64, text: impl Into<String>) -> Result<Self> {
        let s = Self {
            start,
            end,
            text: text.into(),
            timestamp_source: crate::remote::TimestampSource::Unavailable,
        };
        s.validate()?;
        Ok(s)
    }

    /// Construct without validation (trusted provider/postprocess paths and tests).
    ///
    /// Prefer [`Segment::try_new`] for host-facing construction. Callers that
    /// skip validation must treat the segment as untrusted until [`Segment::validate`].
    pub fn from_parts_unchecked(start: f64, end: f64, text: impl Into<String>) -> Self {
        Self {
            start,
            end,
            text: text.into(),
            timestamp_source: crate::remote::TimestampSource::Unavailable,
        }
    }

    /// Unchecked construct with explicit provenance (JOE-2219).
    pub fn from_parts_with_source(
        start: f64,
        end: f64,
        text: impl Into<String>,
        timestamp_source: crate::remote::TimestampSource,
    ) -> Self {
        Self {
            start,
            end,
            text: text.into(),
            timestamp_source,
        }
    }

    pub fn start(&self) -> f64 {
        self.start
    }

    pub fn end(&self) -> f64 {
        self.end
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn timestamp_source(&self) -> crate::remote::TimestampSource {
        self.timestamp_source
    }

    pub fn set_timestamp_source(&mut self, source: crate::remote::TimestampSource) {
        self.timestamp_source = source;
    }

    pub fn set_start(&mut self, start: f64) {
        self.start = start;
    }

    pub fn set_end(&mut self, end: f64) {
        self.end = end;
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
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
/// Fields are **private** (JOE-1809). Prefer builders
/// [`TranscriptionResult::local`] / [`TranscriptionResult::openrouter`] and
/// accessors. `Deserialize` is untrusted — use [`TranscriptionResult::try_from_dto`]
/// or [`TranscriptionResult::validate_segments`] before relying on timings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    text: String,
    segments: Vec<Segment>,
    language: Option<String>,
    model: String,
    provider: String,
    duration_secs: f64,
    /// Backend class — consumers should treat LLM timestamps as best-effort.
    #[serde(default = "default_backend_kind")]
    backend_kind: BackendKind,
    /// Whether segment timestamps are considered reliable.
    #[serde(default = "default_true")]
    timestamps_reliable: bool,
    /// Post-ASR cleanup style applied to [`Self::text`] (default: raw).
    #[serde(default)]
    cleanup_style: crate::cleanup::CleanupStyle,
    /// Cleanup backend used, if any cleanup beyond raw was applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cleanup_provider: Option<crate::cleanup::CleanupProviderKind>,
    /// Pre-cleanup ASR text when cleanup rewrote [`Self::text`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    original_text: Option<String>,
    /// Pre-cleanup ASR segments when cleanup rewrote or cleared timings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    original_segments: Option<Vec<Segment>>,
    /// Segment policy that was applied during cleanup (when not raw).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cleanup_segment_policy: Option<crate::cleanup::SegmentCleanupPolicy>,
}

fn default_backend_kind() -> BackendKind {
    BackendKind::Asr
}
fn default_true() -> bool {
    true
}

impl TranscriptionResult {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn segments_mut(&mut self) -> &mut Vec<Segment> {
        &mut self.segments
    }

    pub fn set_segments(&mut self, segments: Vec<Segment>) {
        self.segments = segments;
    }

    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    pub fn set_language(&mut self, language: Option<String>) {
        self.language = language;
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn set_provider(&mut self, provider: impl Into<String>) {
        self.provider = provider.into();
    }

    pub fn duration_secs(&self) -> f64 {
        self.duration_secs
    }

    pub fn set_duration_secs(&mut self, duration_secs: f64) {
        self.duration_secs = duration_secs;
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.backend_kind
    }

    pub fn set_backend_kind(&mut self, kind: BackendKind) {
        self.backend_kind = kind;
    }

    pub fn timestamps_reliable(&self) -> bool {
        self.timestamps_reliable
    }

    pub fn set_timestamps_reliable(&mut self, reliable: bool) {
        self.timestamps_reliable = reliable;
    }

    /// True when any segment uses approximate/non-native timing (JOE-2219).
    pub fn has_approximate_timestamps(&self) -> bool {
        self.segments
            .iter()
            .any(|s| s.timestamp_source().is_approximate())
    }

    /// Collect segment provenance sources (JOE-2219).
    pub fn timestamp_sources(&self) -> Vec<crate::remote::TimestampSource> {
        self.segments.iter().map(|s| s.timestamp_source()).collect()
    }

    pub fn cleanup_style(&self) -> crate::cleanup::CleanupStyle {
        self.cleanup_style
    }

    pub fn set_cleanup_style(&mut self, style: crate::cleanup::CleanupStyle) {
        self.cleanup_style = style;
    }

    pub fn cleanup_provider(&self) -> Option<crate::cleanup::CleanupProviderKind> {
        self.cleanup_provider
    }

    pub fn set_cleanup_provider(&mut self, provider: Option<crate::cleanup::CleanupProviderKind>) {
        self.cleanup_provider = provider;
    }

    pub fn original_text(&self) -> Option<&str> {
        self.original_text.as_deref()
    }

    pub fn set_original_text(&mut self, text: Option<String>) {
        self.original_text = text;
    }

    pub fn original_segments(&self) -> Option<&[Segment]> {
        self.original_segments.as_deref()
    }

    pub fn set_original_segments(&mut self, segments: Option<Vec<Segment>>) {
        self.original_segments = segments;
    }

    pub fn cleanup_segment_policy(&self) -> Option<crate::cleanup::SegmentCleanupPolicy> {
        self.cleanup_segment_policy
    }

    pub fn set_cleanup_segment_policy(
        &mut self,
        policy: Option<crate::cleanup::SegmentCleanupPolicy>,
    ) {
        self.cleanup_segment_policy = policy;
    }

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

    /// Build a domain result from a public DTO **with validation** (JOE-1809).
    ///
    /// Deserializing JSON into [`crate::dto::SttResultDto`] alone does not create
    /// a trusted domain object — this path re-validates every segment and duration.
    pub fn try_from_dto(dto: &crate::dto::SttResultDto) -> Result<Self> {
        // Accept v1 (pre-provenance) and current v2 (JOE-2219).
        if dto.schema_version != crate::dto::STT_RESULT_SCHEMA_VERSION && dto.schema_version != 1 {
            return Err(crate::error::UserError::Other {
                message: format!(
                    "unsupported STT DTO schema_version {} (expected 1 or {})",
                    dto.schema_version,
                    crate::dto::STT_RESULT_SCHEMA_VERSION
                ),
            }
            .into());
        }
        let mut r = Self {
            text: dto.text.clone(),
            segments: dto.segments.clone(),
            language: dto.language.clone(),
            model: dto.model.clone(),
            provider: dto.provider.clone(),
            duration_secs: dto.duration_secs,
            backend_kind: dto.backend_kind,
            timestamps_reliable: dto.timestamps_reliable,
            cleanup_style: dto.cleanup_style,
            cleanup_provider: dto.cleanup_provider,
            original_text: dto.original_text.clone(),
            original_segments: dto.original_segments.clone(),
            cleanup_segment_policy: dto.cleanup_segment_policy,
        };
        // LLM-assisted paths cannot claim reliable timestamps through DTO injection.
        if matches!(r.backend_kind, BackendKind::LlmAssisted) {
            r.timestamps_reliable = false;
        }
        r.validate_segments()?;
        if let Some(ref segs) = r.original_segments {
            for (i, seg) in segs.iter().enumerate() {
                if let Err(e) = seg.validate() {
                    return Err(crate::error::UserError::Other {
                        message: format!("original_segments[{i}]: {e}"),
                    }
                    .into());
                }
            }
        }
        Ok(r)
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

    /// Like [`Self::local`] but fail-closed when segments/duration are invalid (JOE-1781).
    pub fn try_local(
        text: String,
        segments: Vec<Segment>,
        language: Option<String>,
        model: String,
        duration_secs: f64,
    ) -> Result<Self> {
        let r = Self::local(text, segments, language, model, duration_secs);
        r.validate_segments()?;
        Ok(r)
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

    /// Like [`Self::openrouter`] but fail-closed on invalid segments/duration (JOE-1781).
    pub fn try_openrouter(
        text: String,
        segments: Vec<Segment>,
        language: Option<String>,
        model: String,
        duration_secs: f64,
        timestamps_requested: bool,
    ) -> Result<Self> {
        let r = Self::openrouter(
            text,
            segments,
            language,
            model,
            duration_secs,
            timestamps_requested,
        );
        r.validate_segments()?;
        Ok(r)
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
#[cfg(feature = "tts")]
pub mod openrouter_tts;
#[cfg(feature = "tts")]
pub use openrouter_tts::{
    lookup_openrouter_tts, openrouter_tts_model_in_discovery, OpenRouterTtsProvider,
    OpenRouterTtsRecord, OpenRouterTtsTier, DEFAULT_OPENROUTER_TTS_MODEL,
    DEFAULT_OPENROUTER_TTS_VOICE, OPENROUTER_TTS_EVIDENCE_DATE, OPENROUTER_TTS_REGISTRY,
};

pub mod openai_stt;
pub use openai_stt::{
    lookup_openai_stt, OpenAiSttProvider, OpenAiSttRecord, DEFAULT_OPENAI_STT_MODEL,
    OPENAI_STT_REGISTRY,
};

#[cfg(feature = "tts")]
pub mod openai_tts;
#[cfg(feature = "tts")]
pub use openai_tts::{
    lookup_openai_tts, OpenAiTtsProvider, OpenAiTtsRecord, DEFAULT_OPENAI_TTS_MODEL,
    DEFAULT_OPENAI_TTS_VOICE, OPENAI_TTS_REGISTRY,
};

#[cfg(feature = "tts")]
pub mod elevenlabs_tts;
#[cfg(feature = "tts")]
pub use elevenlabs_tts::{
    lookup_elevenlabs_tts, validate_elevenlabs_voice_id, ElevenLabsTtsProvider,
    ElevenLabsTtsRecord, DEFAULT_ELEVENLABS_TTS_MODEL, ELEVENLABS_TTS_REGISTRY,
    EXAMPLE_ELEVENLABS_VOICE_ID,
};

pub mod xai_stt;
pub use xai_stt::{
    lookup_xai_stt, XaiSttProvider, XaiSttRecord, DEFAULT_XAI_STT_MODEL, XAI_STT_REGISTRY,
};

#[cfg(feature = "tts")]
pub mod xai_tts;
#[cfg(feature = "tts")]
pub use xai_tts::{
    lookup_xai_tts, XaiTtsProvider, XaiTtsRecord, DEFAULT_XAI_TTS_MODEL, DEFAULT_XAI_TTS_VOICE,
    XAI_TTS_REGISTRY,
};

/// Fail-closed: every product default model id must resolve in its reviewed registry.
///
/// Prevents shipping a dead default without an explicit demotion/replace PR (JOE-2213).
#[cfg(test)]
mod registry_defaults_tests {
    use super::*;

    #[test]
    fn product_stt_defaults_resolve_in_reviewed_registries() {
        assert!(
            lookup_openai_stt(DEFAULT_OPENAI_STT_MODEL).is_some(),
            "OpenAI STT default missing from OPENAI_STT_REGISTRY"
        );
        assert!(
            lookup_xai_stt(DEFAULT_XAI_STT_MODEL).is_some(),
            "xAI STT default missing from XAI_STT_REGISTRY"
        );
    }

    #[cfg(feature = "tts")]
    #[test]
    fn product_tts_defaults_resolve_in_reviewed_registries() {
        assert!(
            lookup_openrouter_tts(DEFAULT_OPENROUTER_TTS_MODEL).is_some(),
            "OpenRouter TTS default missing from OPENROUTER_TTS_REGISTRY"
        );
        assert!(
            lookup_openai_tts(DEFAULT_OPENAI_TTS_MODEL).is_some(),
            "OpenAI TTS default missing from OPENAI_TTS_REGISTRY"
        );
        assert!(
            lookup_elevenlabs_tts(DEFAULT_ELEVENLABS_TTS_MODEL).is_some(),
            "ElevenLabs TTS default missing from ELEVENLABS_TTS_REGISTRY"
        );
        assert!(
            lookup_xai_tts(DEFAULT_XAI_TTS_MODEL).is_some(),
            "xAI TTS default missing from XAI_TTS_REGISTRY"
        );
    }
}

/// Default TTS model id when the operator selects a provider without `--model`.
///
/// Local uses the on-device catalogue default. Remote providers use their reviewed
/// registry default so CLI/config local models (e.g. `kitten-nano-int8`) are never
/// sent to OpenRouter/OpenAI/etc.
#[cfg(feature = "tts")]
pub fn default_tts_model_for_provider(provider: &str) -> Option<&'static str> {
    match provider {
        "local" => Some(crate::tts::DEFAULT_TTS_MODEL),
        "openrouter" => Some(DEFAULT_OPENROUTER_TTS_MODEL),
        "openai" => Some(DEFAULT_OPENAI_TTS_MODEL),
        "elevenlabs" => Some(DEFAULT_ELEVENLABS_TTS_MODEL),
        "xai" | "grok" => Some(DEFAULT_XAI_TTS_MODEL),
        _ => None,
    }
}

/// Default TTS voice for a provider when `--voice` is omitted.
///
/// ElevenLabs has no universal default voice id (account-specific); returns `None`.
#[cfg(feature = "tts")]
pub fn default_tts_voice_for_provider(provider: &str) -> Option<&'static str> {
    match provider {
        "local" => Some(crate::tts::DEFAULT_TTS_VOICE),
        "openrouter" => Some(DEFAULT_OPENROUTER_TTS_VOICE),
        "openai" => Some(DEFAULT_OPENAI_TTS_VOICE),
        "elevenlabs" => None,
        "xai" | "grok" => Some(DEFAULT_XAI_TTS_VOICE),
        _ => None,
    }
}

/// Whether `model` is a reviewed id for the given TTS provider (fail closed).
#[cfg(feature = "tts")]
pub fn tts_model_known_for_provider(provider: &str, model: &str) -> bool {
    match provider {
        "local" => crate::tts::lookup_model(model).is_ok(),
        "openrouter" => lookup_openrouter_tts(model).is_some(),
        "openai" => lookup_openai_tts(model).is_some(),
        "elevenlabs" => lookup_elevenlabs_tts(model).is_some(),
        "xai" | "grok" => lookup_xai_tts(model).is_some(),
        _ => false,
    }
}

/// Resolve effective TTS model: explicit CLI wins; otherwise config if valid for
/// the selected provider; otherwise the provider registry default.
#[cfg(feature = "tts")]
pub fn resolve_tts_model(
    provider: &str,
    cli_model: Option<&str>,
    config_model: &str,
) -> Result<String> {
    if let Some(m) = cli_model.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(m.to_string());
    }
    if tts_model_known_for_provider(provider, config_model) {
        return Ok(config_model.to_string());
    }
    if let Some(d) = default_tts_model_for_provider(provider) {
        return Ok(d.to_string());
    }
    Err(crate::error::UserError::UnsupportedCapability {
        provider: provider.into(),
        model: config_model.into(),
        reason: "no reviewed default TTS model for this provider".into(),
        hint: "pass --model with a reviewed id for the selected provider".into(),
    }
    .into())
}

/// Resolve effective TTS voice: explicit CLI wins; then config if non-empty for
/// local; otherwise provider default when available.
#[cfg(feature = "tts")]
pub fn resolve_tts_voice(
    provider: &str,
    cli_voice: Option<&str>,
    config_voice: &str,
) -> Result<String> {
    if let Some(v) = cli_voice.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(v.to_string());
    }
    match provider {
        "local" => {
            let v = config_voice.trim();
            if !v.is_empty() {
                Ok(v.to_string())
            } else {
                Ok(crate::tts::DEFAULT_TTS_VOICE.to_string())
            }
        }
        "elevenlabs" => Err(crate::error::UserError::Other {
            message: "ElevenLabs requires an explicit --voice <voice_id> (no local alias remap)"
                .into(),
        }
        .into()),
        other => {
            if let Some(d) = default_tts_voice_for_provider(other) {
                Ok(d.to_string())
            } else {
                let v = config_voice.trim();
                if !v.is_empty() {
                    Ok(v.to_string())
                } else {
                    Err(crate::error::UserError::Other {
                        message: format!("TTS voice is required for provider '{other}'"),
                    }
                    .into())
                }
            }
        }
    }
}

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

    #[test]
    fn try_local_rejects_nan_segment() {
        let segs = vec![Segment::from_parts_unchecked(
            f64::NAN,
            1.0,
            "x".to_string(),
        )];
        assert!(TranscriptionResult::try_local("x".into(), segs, None, "m".into(), 1.0).is_err());
    }

    #[test]
    fn try_local_accepts_valid() {
        let segs = vec![Segment::try_new(0.0, 0.5, "hi").unwrap()];
        let r =
            TranscriptionResult::try_local("hi".into(), segs, Some("en".into()), "m".into(), 1.0)
                .unwrap();
        assert_eq!(r.provider(), "local");
    }

    #[test]
    fn try_from_dto_rejects_nan_segment() {
        let mut dto = crate::dto::SttResultDto::from_result(&TranscriptionResult::local(
            "x".into(),
            vec![Segment::try_new(0.0, 1.0, "x").unwrap()],
            None,
            "m".into(),
            1.0,
        ));
        dto.segments = vec![Segment::from_parts_unchecked(
            f64::NAN,
            1.0,
            "x".to_string(),
        )];
        assert!(TranscriptionResult::try_from_dto(&dto).is_err());
    }

    #[test]
    fn try_from_dto_forces_llm_timestamps_unreliable() {
        let mut dto = crate::dto::SttResultDto::from_result(&TranscriptionResult::openrouter(
            "hi".into(),
            vec![Segment::try_new(0.0, 1.0, "hi").unwrap()],
            None,
            "m".into(),
            1.0,
            true,
        ));
        dto.timestamps_reliable = true; // injection attempt
        let r = TranscriptionResult::try_from_dto(&dto).unwrap();
        assert!(!r.timestamps_reliable());
        assert_eq!(r.backend_kind(), BackendKind::LlmAssisted);
    }
}
