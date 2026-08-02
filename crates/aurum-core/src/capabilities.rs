//! Provider/model capability contracts and preflight routing (JOE-1613 / JOE-1829 / JOE-1936).
//!
//! Capabilities are declared for built-ins and consulted before expensive work
//! (decode, download, network). OpenRouter `auto` routing is **capability-
//! authoritative**: only models in the reviewed static registry are auto-routed;
//! unknown models fail closed and require an explicit `chat` or `transcriptions`
//! mode — never silent model-name heuristics.
//!
//! # Ownership (JOE-1936)
//!
//! Factories on [`crate::provider_platform::ProviderRegistry`] are the preferred
//! lookup path via [`crate::provider_platform::capabilities_for`]. Free functions
//! below remain the canonical **built-in** descriptors that factories call, so
//! request-path preflight and discovery stay consistent without duplicating
//! tables in the CLI.

use crate::error::{Result, UserError};
use crate::provider_platform::ProviderId;
use crate::providers::OpenRouterSttMode;
use serde::{Deserialize, Serialize};

/// Schema version for capability JSON.
///
/// v1 remains the wire version: remote TTS/STT fields are **optional** with
/// defaults so older readers ignore unknowns and older writers remain valid.
pub const CAPABILITY_SCHEMA_VERSION: u32 = 1;

/// Backend class for STT honesty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SttBackendClass {
    Asr,
    LlmAssisted,
}

/// How fresh / trusted a capability descriptor is.
///
/// Remote refresh is an explicit network action (not done by default). Labels
/// keep discovery honest about whether values are compile-time static or
/// human-reviewed catalogue entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DescriptorFreshness {
    /// Compile-time / code-owned declaration (default for free functions).
    #[default]
    Static,
    /// Human-reviewed catalogue entry (e.g. OpenRouter auto-route table).
    Reviewed,
}

/// Voice selection model advertised by a TTS (or multimodal) provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceModel {
    /// Fixed set of named aliases (local Kitten voices, etc.).
    FixedAliases,
    /// Provider-assigned opaque voice identifiers.
    ProviderVoiceIds,
    /// No selectable voice (single-speaker or STT/cleanup).
    None,
}

/// Declared capabilities for a provider/model combination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderCapabilities {
    pub schema_version: u32,
    pub provider: String,
    pub model: String,
    pub operation: CapabilityOperation,
    /// STT backend class when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stt_backend: Option<SttBackendClass>,
    pub timestamps_reliable: bool,
    pub languages: Vec<String>,
    pub max_duration_secs: Option<f64>,
    pub max_upload_bytes: Option<u64>,
    pub max_text_chars: Option<usize>,
    pub supports_cancellation: bool,
    pub requires_network: bool,
    pub local_only_ok: bool,
    /// High-level product output containers (txt/srt/json/wav), not raw codecs.
    pub output_formats: Vec<String>,
    pub notes: Vec<String>,

    // ── Optional remote / audio-semantic extensions (JOE-1936, schema v1) ──
    /// How voices are selected when `operation` is TTS (or multimodal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_model: Option<VoiceModel>,
    /// Whether the provider accepts opaque provider-native voice ids.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub supports_voice_ids: bool,
    /// Accepted input audio containers/encodings for STT (e.g. `wav`, `mp3`, `pcm_s16le`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepted_audio_formats: Vec<String>,
    /// Output audio containers/encodings for TTS (e.g. `wav`, `mp3`, `pcm_f32le`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_audio_formats: Vec<String>,
    /// Native PCM samples available without a container (library embed path).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub direct_pcm: bool,
    /// Sample rates the provider advertises (Hz). Empty = unspecified / native-only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sample_rates_hz: Vec<u32>,
    /// Inclusive speaking-rate minimum when rate is supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaking_rate_min: Option<f32>,
    /// Inclusive speaking-rate maximum when rate is supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaking_rate_max: Option<f32>,
    /// Whether speaking-rate is an accepted request parameter.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub supports_speaking_rate: bool,
    /// Provider (or wire protocol) claims streaming is available.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub streaming_advertised: bool,
    /// Aurum actually implements streaming for this vertical today.
    ///
    /// Deliberately separate from [`Self::streaming_advertised`]: advertising
    /// alone must not imply product support.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub streaming_implemented_by_aurum: bool,
    /// Freshness / trust label for this descriptor.
    #[serde(default, skip_serializing_if = "is_default_freshness")]
    pub descriptor_freshness: DescriptorFreshness,
}

fn is_default_freshness(f: &DescriptorFreshness) -> bool {
    *f == DescriptorFreshness::Static
}

impl ProviderCapabilities {
    /// Extension fields left at honest defaults (empty / false / static).
    pub fn with_core(
        provider: impl Into<String>,
        model: impl Into<String>,
        operation: CapabilityOperation,
    ) -> Self {
        Self {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            provider: provider.into(),
            model: model.into(),
            operation,
            stt_backend: None,
            timestamps_reliable: false,
            languages: Vec::new(),
            max_duration_secs: None,
            max_upload_bytes: None,
            max_text_chars: None,
            supports_cancellation: false,
            requires_network: false,
            local_only_ok: true,
            output_formats: Vec::new(),
            notes: Vec::new(),
            voice_model: None,
            supports_voice_ids: false,
            accepted_audio_formats: Vec::new(),
            output_audio_formats: Vec::new(),
            direct_pcm: false,
            sample_rates_hz: Vec::new(),
            speaking_rate_min: None,
            speaking_rate_max: None,
            supports_speaking_rate: false,
            streaming_advertised: false,
            streaming_implemented_by_aurum: false,
            descriptor_freshness: DescriptorFreshness::Static,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityOperation {
    Stt,
    Tts,
    Cleanup,
}

/// Why a request was rejected at preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedCapability {
    pub provider: String,
    pub model: String,
    pub reason: String,
    pub hint: String,
}

impl std::fmt::Display for UnsupportedCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unsupported capability for {}/{}: {}\n  Hint: {}",
            self.provider, self.model, self.reason, self.hint
        )
    }
}

impl From<UnsupportedCapability> for crate::error::TranscriptionError {
    fn from(u: UnsupportedCapability) -> Self {
        UserError::UnsupportedCapability {
            provider: u.provider,
            model: u.model,
            reason: u.reason,
            hint: u.hint,
        }
        .into()
    }
}

/// Local Whisper STT capabilities.
pub fn local_whisper_capabilities(model: &str) -> ProviderCapabilities {
    let mut caps = ProviderCapabilities::with_core("local", model, CapabilityOperation::Stt);
    caps.stt_backend = Some(SttBackendClass::Asr);
    caps.timestamps_reliable = true;
    caps.languages = vec!["auto".into(), "en".into(), "multilingual".into()];
    caps.max_duration_secs = Some(2.5 * 3600.0);
    caps.supports_cancellation = true;
    caps.requires_network = false; // download optional when cached
    caps.local_only_ok = true;
    caps.output_formats = vec!["txt".into(), "srt".into(), "json".into()];
    caps.accepted_audio_formats = vec![
        "wav".into(),
        "mp3".into(),
        "flac".into(),
        "m4a".into(),
        "ogg".into(),
        "pcm_f32le".into(),
    ];
    caps.direct_pcm = true;
    caps.sample_rates_hz = vec![16_000];
    caps.descriptor_freshness = DescriptorFreshness::Static;
    caps.notes = vec![
        "Timestamps are engine-derived ASR timings.".into(),
        "Network only needed when the model is not cached.".into(),
    ];
    caps
}

/// OpenRouter STT capabilities after path resolution.
pub fn openrouter_stt_capabilities(model: &str, path: OpenRouterSttPath) -> ProviderCapabilities {
    match path {
        OpenRouterSttPath::Transcriptions => {
            let mut caps =
                ProviderCapabilities::with_core("openrouter", model, CapabilityOperation::Stt);
            caps.stt_backend = Some(SttBackendClass::Asr);
            caps.timestamps_reliable = true;
            caps.languages = vec!["auto".into()];
            caps.max_duration_secs = Some(3600.0);
            caps.max_upload_bytes = Some(24 * 1024 * 1024);
            caps.supports_cancellation = false;
            caps.requires_network = true;
            caps.local_only_ok = false;
            caps.output_formats = vec!["txt".into(), "srt".into(), "json".into()];
            caps.accepted_audio_formats = vec![
                "wav".into(),
                "mp3".into(),
                "flac".into(),
                "m4a".into(),
                "ogg".into(),
            ];
            caps.descriptor_freshness = DescriptorFreshness::Reviewed;
            caps.notes = vec!["Dedicated /audio/transcriptions ASR path.".into()];
            caps
        }
        OpenRouterSttPath::Chat => {
            let mut caps =
                ProviderCapabilities::with_core("openrouter", model, CapabilityOperation::Stt);
            caps.stt_backend = Some(SttBackendClass::LlmAssisted);
            caps.timestamps_reliable = false;
            caps.languages = vec!["auto".into()];
            caps.max_duration_secs = Some(3600.0);
            caps.max_upload_bytes = Some(24 * 1024 * 1024);
            caps.supports_cancellation = false;
            caps.requires_network = true;
            caps.local_only_ok = false;
            caps.output_formats = vec!["txt".into(), "json".into()];
            caps.accepted_audio_formats = vec![
                "wav".into(),
                "mp3".into(),
                "flac".into(),
                "m4a".into(),
                "ogg".into(),
            ];
            caps.descriptor_freshness = DescriptorFreshness::Reviewed;
            caps.notes = vec![
                "LLM-assisted multimodal chat path; timestamps are unreliable.".into(),
                "Do not use SRT when timestamps_reliable is false.".into(),
            ];
            caps
        }
    }
}

/// Resolved OpenRouter STT route (capability-facing name for SttPath).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRouterSttPath {
    Transcriptions,
    Chat,
}

/// Reviewed static capability record for an OpenRouter STT model id (JOE-1829).
///
/// `auto` routing consults this table only — never model-name substring guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenRouterSttRecord {
    /// Canonical OpenRouter model slug (lowercase).
    pub model_id: &'static str,
    pub path: OpenRouterSttPath,
    pub backend: SttBackendClass,
    pub timestamps_reliable: bool,
}

/// Authoritative registry of OpenRouter models Aurum will auto-route.
///
/// Explicit `chat` / `transcriptions` modes still accept any model id; `auto`
/// requires a match here so unfamiliar names fail closed.
pub static OPENROUTER_STT_REGISTRY: &[OpenRouterSttRecord] = &[
    OpenRouterSttRecord {
        model_id: "openai/whisper-1",
        path: OpenRouterSttPath::Transcriptions,
        backend: SttBackendClass::Asr,
        timestamps_reliable: true,
    },
    OpenRouterSttRecord {
        model_id: "openai/whisper-large-v3",
        path: OpenRouterSttPath::Transcriptions,
        backend: SttBackendClass::Asr,
        timestamps_reliable: true,
    },
    OpenRouterSttRecord {
        model_id: "openai/whisper-large-v3-turbo",
        path: OpenRouterSttPath::Transcriptions,
        backend: SttBackendClass::Asr,
        timestamps_reliable: true,
    },
    OpenRouterSttRecord {
        model_id: "openai/gpt-4o-transcribe",
        path: OpenRouterSttPath::Transcriptions,
        backend: SttBackendClass::Asr,
        timestamps_reliable: true,
    },
    OpenRouterSttRecord {
        model_id: "openai/gpt-4o-mini-transcribe",
        path: OpenRouterSttPath::Transcriptions,
        backend: SttBackendClass::Asr,
        timestamps_reliable: true,
    },
    OpenRouterSttRecord {
        model_id: "google/gemini-2.5-flash",
        path: OpenRouterSttPath::Chat,
        backend: SttBackendClass::LlmAssisted,
        timestamps_reliable: false,
    },
    OpenRouterSttRecord {
        model_id: "google/gemini-2.5-flash-lite",
        path: OpenRouterSttPath::Chat,
        backend: SttBackendClass::LlmAssisted,
        timestamps_reliable: false,
    },
    OpenRouterSttRecord {
        model_id: "google/gemini-2.5-pro",
        path: OpenRouterSttPath::Chat,
        backend: SttBackendClass::LlmAssisted,
        timestamps_reliable: false,
    },
    OpenRouterSttRecord {
        model_id: "google/gemini-2.0-flash",
        path: OpenRouterSttPath::Chat,
        backend: SttBackendClass::LlmAssisted,
        timestamps_reliable: false,
    },
    OpenRouterSttRecord {
        model_id: "openai/gpt-4o",
        path: OpenRouterSttPath::Chat,
        backend: SttBackendClass::LlmAssisted,
        timestamps_reliable: false,
    },
    OpenRouterSttRecord {
        model_id: "openai/gpt-4o-mini",
        path: OpenRouterSttPath::Chat,
        backend: SttBackendClass::LlmAssisted,
        timestamps_reliable: false,
    },
    OpenRouterSttRecord {
        model_id: "openai/gpt-4o-audio-preview",
        path: OpenRouterSttPath::Chat,
        backend: SttBackendClass::LlmAssisted,
        timestamps_reliable: false,
    },
    OpenRouterSttRecord {
        model_id: "openai/gpt-audio-mini",
        path: OpenRouterSttPath::Chat,
        backend: SttBackendClass::LlmAssisted,
        timestamps_reliable: false,
    },
    OpenRouterSttRecord {
        model_id: "mistralai/voxtral-small-24b-2507",
        path: OpenRouterSttPath::Chat,
        backend: SttBackendClass::LlmAssisted,
        timestamps_reliable: false,
    },
];

/// Look up a reviewed OpenRouter STT capability record (exact id, case-insensitive).
pub fn lookup_openrouter_stt(model: &str) -> Option<&'static OpenRouterSttRecord> {
    let m = model.trim().to_ascii_lowercase();
    if m.is_empty() {
        return None;
    }
    OPENROUTER_STT_REGISTRY
        .iter()
        .find(|r| r.model_id == m.as_str())
}

/// Route OpenRouter STT from explicit mode + **reviewed** capability registry.
pub fn resolve_openrouter_stt_path(
    mode: OpenRouterSttMode,
    model: &str,
) -> Result<OpenRouterSttPath> {
    match mode {
        OpenRouterSttMode::Chat => Ok(OpenRouterSttPath::Chat),
        OpenRouterSttMode::Transcriptions => Ok(OpenRouterSttPath::Transcriptions),
        OpenRouterSttMode::Auto => match lookup_openrouter_stt(model) {
            Some(rec) => Ok(rec.path),
            None => Err(UnsupportedCapability {
                provider: "openrouter".into(),
                model: model.trim().into(),
                reason: "auto routing has no reviewed capability record for this model".into(),
                hint: "set openrouter_stt_mode=chat or transcriptions explicitly, or use a \
                       registered model id (see aurum capabilities / OPENROUTER_STT_REGISTRY)"
                    .into(),
            }
            .into()),
        },
    }
}

/// Rules cleanup capabilities (language-aware).
pub fn rules_cleanup_capabilities(language: &str) -> ProviderCapabilities {
    let lang = language.trim().to_ascii_lowercase();
    let english = lang.is_empty()
        || lang == "auto"
        || lang == "en"
        || lang.starts_with("en-")
        || lang == "eng";
    let mut caps =
        ProviderCapabilities::with_core("rules", "builtin", CapabilityOperation::Cleanup);
    caps.languages = if english {
        vec!["en".into(), "auto".into()]
    } else {
        vec![lang]
    };
    caps.max_text_chars = Some(500_000);
    caps.supports_cancellation = true;
    caps.requires_network = false;
    caps.local_only_ok = true;
    caps.output_formats = vec!["txt".into(), "json".into()];
    caps.descriptor_freshness = DescriptorFreshness::Static;
    caps.notes = if english {
        vec!["English filler/contraction heuristics apply for clean/professional.".into()]
    } else {
        vec!["Non-English: only whitespace-safe normalization; no English filler deletion.".into()]
    };
    caps
}

/// Local Kitten TTS capabilities.
pub fn local_tts_capabilities(model: &str) -> ProviderCapabilities {
    let mut caps = ProviderCapabilities::with_core("local", model, CapabilityOperation::Tts);
    caps.languages = vec!["en".into()];
    caps.max_text_chars = Some(5_000);
    caps.supports_cancellation = true;
    caps.requires_network = false;
    caps.local_only_ok = true;
    caps.output_formats = vec!["wav".into(), "json".into()];
    caps.voice_model = Some(VoiceModel::FixedAliases);
    caps.supports_voice_ids = false;
    caps.output_audio_formats = vec!["wav".into(), "pcm_f32le".into()];
    caps.direct_pcm = true;
    caps.sample_rates_hz = vec![24_000];
    caps.supports_speaking_rate = true;
    caps.speaking_rate_min = Some(0.5);
    caps.speaking_rate_max = Some(2.0);
    caps.streaming_advertised = false;
    caps.streaming_implemented_by_aurum = false;
    caps.descriptor_freshness = DescriptorFreshness::Static;
    caps.notes = vec!["English KittenTTS ONNX path only.".into()];
    caps
}

/// OpenRouter remote TTS capabilities (JOE-1939). Fail closed when model is unknown.
pub fn openrouter_tts_capabilities(model: &str) -> Result<ProviderCapabilities> {
    #[cfg(feature = "tts")]
    {
        use crate::providers::openrouter_tts::lookup_openrouter_tts;
        let rec = lookup_openrouter_tts(model).ok_or_else(|| UnsupportedCapability {
            provider: "openrouter".into(),
            model: model.into(),
            reason: "model is not in the reviewed OpenRouter TTS registry".into(),
            hint: "use a reviewed OpenRouter TTS model (see docs/guide/providers.md)".into(),
        })?;
        let mut caps =
            ProviderCapabilities::with_core("openrouter", rec.model, CapabilityOperation::Tts);
        caps.languages = vec!["en".into(), "auto".into()];
        caps.max_text_chars = Some(rec.max_text_chars);
        caps.supports_cancellation = true;
        caps.requires_network = true;
        caps.local_only_ok = false;
        caps.output_formats = vec!["wav".into(), "json".into()];
        caps.voice_model = Some(VoiceModel::ProviderVoiceIds);
        caps.supports_voice_ids = true;
        caps.output_audio_formats = vec!["pcm_s16le".into(), "mp3".into()];
        caps.direct_pcm = true;
        caps.sample_rates_hz = vec![rec.default_sample_rate_hz];
        caps.supports_speaking_rate = true;
        caps.speaking_rate_min = Some(rec.rate_min);
        caps.speaking_rate_max = Some(rec.rate_max);
        caps.streaming_advertised = false;
        caps.streaming_implemented_by_aurum = false;
        caps.descriptor_freshness = DescriptorFreshness::Reviewed;
        caps.notes = vec![
            "OpenRouter dedicated /audio/speech endpoint (OpenAI-compatible).".into(),
            "Text is transmitted to OpenRouter / upstream; network privacy applies.".into(),
            format!("Reviewed voices: {}.", rec.voices.join(", ")),
        ];
        Ok(caps)
    }
    #[cfg(not(feature = "tts"))]
    {
        let _ = model;
        Err(UserError::Other {
            message: "TTS support is not compiled into this build (feature `tts`)".into(),
        }
        .into())
    }
}

/// Preflight STT: reject offline OpenRouter, unreliable SRT, unknown providers.
pub fn preflight_stt(
    provider: &str,
    model: &str,
    want_srt: bool,
    local_only: bool,
    stt_mode: OpenRouterSttMode,
) -> Result<ProviderCapabilities> {
    match provider {
        "local" => Ok(local_whisper_capabilities(model)),
        "openrouter" => {
            if local_only {
                return Err(UnsupportedCapability {
                    provider: provider.into(),
                    model: model.into(),
                    reason: "OpenRouter requires network access".into(),
                    hint: "unset local_only or use provider=local with a cached model".into(),
                }
                .into());
            }
            let path = resolve_openrouter_stt_path(stt_mode, model)?;
            let mut caps = openrouter_stt_capabilities(model, path);
            if let Some(rec) = lookup_openrouter_stt(model) {
                caps.stt_backend = Some(rec.backend);
                caps.timestamps_reliable = rec.timestamps_reliable;
                caps.descriptor_freshness = DescriptorFreshness::Reviewed;
                if !caps.notes.iter().any(|n| n.contains("registry")) {
                    caps.notes.push(format!(
                        "Routed via reviewed capability registry → {}.",
                        match rec.path {
                            OpenRouterSttPath::Transcriptions => "transcriptions",
                            OpenRouterSttPath::Chat => "chat",
                        }
                    ));
                }
            }
            if want_srt && !caps.timestamps_reliable {
                return Err(UnsupportedCapability {
                    provider: provider.into(),
                    model: model.into(),
                    reason: "SRT requires reliable timestamps; this model path is LLM-assisted"
                        .into(),
                    hint: "use openrouter_stt_mode=transcriptions with a dedicated ASR model, or output txt/json"
                        .into(),
                }
                .into());
            }
            Ok(caps)
        }
        other => Err(UserError::InvalidProvider {
            provider: other.into(),
        }
        .into()),
    }
}

/// [`preflight_stt`] with a validated [`ProviderId`].
pub fn preflight_stt_for(
    provider: &ProviderId,
    model: &str,
    want_srt: bool,
    local_only: bool,
    stt_mode: OpenRouterSttMode,
) -> Result<ProviderCapabilities> {
    preflight_stt(provider.as_str(), model, want_srt, local_only, stt_mode)
}

/// Preflight local TTS language support (legacy string API; assumes `local`).
pub fn preflight_tts(language: &str, local_only: bool) -> Result<ProviderCapabilities> {
    preflight_tts_for(&ProviderId::local(), language, local_only)
}

/// Preflight TTS for a validated provider id.
pub fn preflight_tts_for(
    provider: &ProviderId,
    language: &str,
    local_only: bool,
) -> Result<ProviderCapabilities> {
    match provider.as_str() {
        "local" => {
            let caps = local_tts_capabilities("kitten-nano-int8");
            if local_only && (caps.requires_network || !caps.local_only_ok) {
                return Err(UnsupportedCapability {
                    provider: provider.as_str().into(),
                    model: caps.model.clone(),
                    reason: "TTS provider requires network under local_only".into(),
                    hint: "use a local TTS model or unset local_only".into(),
                }
                .into());
            }
            let lang = language.trim().to_ascii_lowercase();
            if !(lang.is_empty() || lang == "en" || lang.starts_with("en-")) {
                return Err(UnsupportedCapability {
                    provider: "local".into(),
                    model: caps.model.clone(),
                    reason: format!("TTS language '{language}' is not supported"),
                    hint: "use language=en for KittenTTS".into(),
                }
                .into());
            }
            Ok(caps)
        }
        "openrouter" => {
            #[cfg(feature = "tts")]
            {
                if local_only {
                    return Err(UnsupportedCapability {
                        provider: provider.as_str().into(),
                        model: "*".into(),
                        reason: "OpenRouter TTS requires network access".into(),
                        hint: "unset local_only or use provider=local".into(),
                    }
                    .into());
                }
                // Model-specific checks run at synthesize; gate network here.
                openrouter_tts_capabilities(
                    crate::providers::openrouter_tts::DEFAULT_OPENROUTER_TTS_MODEL,
                )
            }
            #[cfg(not(feature = "tts"))]
            {
                let _ = (provider, language, local_only);
                Err(UserError::Other {
                    message: "TTS support is not compiled into this build (feature `tts`)".into(),
                }
                .into())
            }
        }
        other => Err(UserError::InvalidProvider {
            provider: other.into(),
        }
        .into()),
    }
}

pub fn preflight_cleanup(
    provider: &str,
    language: &str,
    style: &str,
) -> Result<ProviderCapabilities> {
    match provider {
        "rules" | "local" => {
            let caps = rules_cleanup_capabilities(language);
            let _style = style.trim().to_ascii_lowercase();
            let _lang = language.trim().to_ascii_lowercase();
            Ok(caps)
        }
        "openrouter" => {
            let mut caps =
                ProviderCapabilities::with_core("openrouter", "chat", CapabilityOperation::Cleanup);
            caps.languages = vec!["auto".into()];
            caps.max_text_chars = Some(100_000);
            caps.supports_cancellation = false;
            caps.requires_network = true;
            caps.local_only_ok = false;
            caps.output_formats = vec!["txt".into(), "json".into()];
            caps.descriptor_freshness = DescriptorFreshness::Static;
            caps.notes = vec!["LLM rewrite; not deterministic.".into()];
            Ok(caps)
        }
        other => Err(UserError::Other {
            message: format!("unknown cleanup provider '{other}'"),
        }
        .into()),
    }
}

/// [`preflight_cleanup`] with a validated [`ProviderId`].
pub fn preflight_cleanup_for(
    provider: &ProviderId,
    language: &str,
    style: &str,
) -> Result<ProviderCapabilities> {
    preflight_cleanup(provider.as_str(), language, style)
}

/// Apply common request gates against already-resolved capabilities (fail closed).
pub fn apply_stt_request_gates(
    caps: &ProviderCapabilities,
    want_srt: bool,
    local_only: bool,
) -> Result<()> {
    if local_only && (caps.requires_network || !caps.local_only_ok) {
        return Err(UnsupportedCapability {
            provider: caps.provider.clone(),
            model: caps.model.clone(),
            reason: "provider requires network access under local_only".into(),
            hint: "unset local_only or use a local-only provider with a cached model".into(),
        }
        .into());
    }
    if want_srt && !caps.timestamps_reliable {
        return Err(UnsupportedCapability {
            provider: caps.provider.clone(),
            model: caps.model.clone(),
            reason: "SRT requires reliable timestamps".into(),
            hint: "use an ASR model with timestamps_reliable=true, or output txt/json".into(),
        }
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_blocked_for_llm_chat() {
        let err = preflight_stt(
            "openrouter",
            "google/gemini-2.5-flash",
            true,
            false,
            OpenRouterSttMode::Auto,
        )
        .unwrap_err();
        assert!(err.to_string().contains("SRT") || err.to_string().contains("timestamp"));
    }

    #[test]
    fn whisper_auto_routes_transcriptions() {
        assert_eq!(
            resolve_openrouter_stt_path(OpenRouterSttMode::Auto, "openai/whisper-large-v3")
                .unwrap(),
            OpenRouterSttPath::Transcriptions
        );
    }

    #[test]
    fn gemini_auto_routes_chat() {
        assert_eq!(
            resolve_openrouter_stt_path(OpenRouterSttMode::Auto, "google/gemini-2.5-flash")
                .unwrap(),
            OpenRouterSttPath::Chat
        );
    }

    #[test]
    fn auto_unknown_model_fails_closed() {
        let err =
            resolve_openrouter_stt_path(OpenRouterSttMode::Auto, "acme/totally-unknown-asr-v99")
                .unwrap_err();
        let s = err.to_string();
        assert!(
            s.contains("reviewed capability") || s.contains("unsupported"),
            "unexpected error: {s}"
        );
        assert!(lookup_openrouter_stt("acme/totally-unknown-asr-v99").is_none());
    }

    #[test]
    fn auto_does_not_guess_whisper_substring() {
        let err = resolve_openrouter_stt_path(
            OpenRouterSttMode::Auto,
            "vendor/whisper-clone-experimental",
        )
        .unwrap_err();
        assert!(err.to_string().contains("reviewed") || err.to_string().contains("unsupported"));
    }

    #[test]
    fn explicit_mode_accepts_unregistered() {
        assert_eq!(
            resolve_openrouter_stt_path(OpenRouterSttMode::Transcriptions, "vendor/custom-asr")
                .unwrap(),
            OpenRouterSttPath::Transcriptions
        );
        assert_eq!(
            resolve_openrouter_stt_path(OpenRouterSttMode::Chat, "vendor/custom-llm").unwrap(),
            OpenRouterSttPath::Chat
        );
    }

    #[test]
    fn registry_records_are_unique_lowercase() {
        let mut seen = std::collections::HashSet::new();
        for rec in OPENROUTER_STT_REGISTRY {
            assert_eq!(rec.model_id, rec.model_id.to_ascii_lowercase());
            assert!(
                seen.insert(rec.model_id),
                "duplicate registry model_id: {}",
                rec.model_id
            );
            assert_eq!(
                rec.timestamps_reliable,
                matches!(rec.path, OpenRouterSttPath::Transcriptions)
            );
            assert_eq!(
                rec.backend,
                match rec.path {
                    OpenRouterSttPath::Transcriptions => SttBackendClass::Asr,
                    OpenRouterSttPath::Chat => SttBackendClass::LlmAssisted,
                }
            );
        }
    }

    #[test]
    fn offline_openrouter_fails() {
        let err = preflight_stt(
            "openrouter",
            "openai/whisper-large-v3",
            false,
            true,
            OpenRouterSttMode::Transcriptions,
        )
        .unwrap_err();
        assert!(err.to_string().contains("network") || err.to_string().contains("OpenRouter"));
    }

    #[test]
    fn tts_rejects_french() {
        assert!(preflight_tts("fr", true).is_err());
        assert!(preflight_tts("en", true).is_ok());
    }

    #[test]
    fn preflight_accepts_provider_id() {
        let local = ProviderId::local();
        let caps =
            preflight_stt_for(&local, "tiny-q5_1", false, true, OpenRouterSttMode::Auto).unwrap();
        assert_eq!(caps.provider, "local");
        assert!(caps.direct_pcm);

        let or = ProviderId::openrouter();
        assert!(preflight_stt_for(
            &or,
            "openai/whisper-large-v3",
            false,
            true,
            OpenRouterSttMode::Transcriptions,
        )
        .is_err());
    }

    #[test]
    fn capability_json_no_secrets_and_optional_fields_roundtrip() {
        let caps = local_whisper_capabilities("base");
        let s = serde_json::to_string(&caps).unwrap();
        assert!(!s.contains("sk-"));
        assert!(s.contains("schema_version"));
        let back: ProviderCapabilities = serde_json::from_str(&s).unwrap();
        assert_eq!(back.schema_version, CAPABILITY_SCHEMA_VERSION);
        assert_eq!(back.sample_rates_hz, vec![16_000]);

        let legacy = r#"{
            "schema_version": 1,
            "provider": "local",
            "model": "base",
            "operation": "stt",
            "timestamps_reliable": true,
            "languages": ["en"],
            "max_duration_secs": null,
            "max_upload_bytes": null,
            "max_text_chars": null,
            "supports_cancellation": true,
            "requires_network": false,
            "local_only_ok": true,
            "output_formats": ["txt"],
            "notes": []
        }"#;
        let legacy_caps: ProviderCapabilities = serde_json::from_str(legacy).unwrap();
        assert!(!legacy_caps.supports_voice_ids);
        assert!(legacy_caps.accepted_audio_formats.is_empty());
        assert_eq!(
            legacy_caps.descriptor_freshness,
            DescriptorFreshness::Static
        );
    }

    #[test]
    fn local_tts_declares_rate_and_pcm() {
        let caps = local_tts_capabilities("kitten-nano-int8");
        assert!(caps.supports_speaking_rate);
        assert_eq!(caps.speaking_rate_min, Some(0.5));
        assert_eq!(caps.speaking_rate_max, Some(2.0));
        assert!(caps.direct_pcm);
        assert_eq!(caps.voice_model, Some(VoiceModel::FixedAliases));
        assert!(!caps.streaming_implemented_by_aurum);
    }
}
