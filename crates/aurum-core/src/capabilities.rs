//! Provider/model capability contracts and preflight routing (JOE-1613 / JOE-1829).
//!
//! Capabilities are declared for built-ins and consulted before expensive work
//! (decode, download, network). OpenRouter `auto` routing is **capability-
//! authoritative**: only models in the reviewed static registry are auto-routed;
//! unknown models fail closed and require an explicit `chat` or `transcriptions`
//! mode — never silent model-name heuristics.

use crate::error::{Result, UserError};
use crate::providers::OpenRouterSttMode;
use serde::{Deserialize, Serialize};

/// Schema version for capability JSON.
pub const CAPABILITY_SCHEMA_VERSION: u32 = 1;

/// Backend class for STT honesty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SttBackendClass {
    Asr,
    LlmAssisted,
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
    pub output_formats: Vec<String>,
    pub notes: Vec<String>,
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
    ProviderCapabilities {
        schema_version: CAPABILITY_SCHEMA_VERSION,
        provider: "local".into(),
        model: model.into(),
        operation: CapabilityOperation::Stt,
        stt_backend: Some(SttBackendClass::Asr),
        timestamps_reliable: true,
        languages: vec!["auto".into(), "en".into(), "multilingual".into()],
        max_duration_secs: Some(2.5 * 3600.0),
        max_upload_bytes: None,
        max_text_chars: None,
        supports_cancellation: true,
        requires_network: false, // download optional when cached
        local_only_ok: true,
        output_formats: vec!["txt".into(), "srt".into(), "json".into()],
        notes: vec![
            "Timestamps are engine-derived ASR timings.".into(),
            "Network only needed when the model is not cached.".into(),
        ],
    }
}

/// OpenRouter STT capabilities after path resolution.
pub fn openrouter_stt_capabilities(model: &str, path: OpenRouterSttPath) -> ProviderCapabilities {
    match path {
        OpenRouterSttPath::Transcriptions => ProviderCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            provider: "openrouter".into(),
            model: model.into(),
            operation: CapabilityOperation::Stt,
            stt_backend: Some(SttBackendClass::Asr),
            timestamps_reliable: true,
            languages: vec!["auto".into()],
            max_duration_secs: Some(3600.0),
            max_upload_bytes: Some(24 * 1024 * 1024),
            max_text_chars: None,
            supports_cancellation: false,
            requires_network: true,
            local_only_ok: false,
            output_formats: vec!["txt".into(), "srt".into(), "json".into()],
            notes: vec!["Dedicated /audio/transcriptions ASR path.".into()],
        },
        OpenRouterSttPath::Chat => ProviderCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            provider: "openrouter".into(),
            model: model.into(),
            operation: CapabilityOperation::Stt,
            stt_backend: Some(SttBackendClass::LlmAssisted),
            timestamps_reliable: false,
            languages: vec!["auto".into()],
            max_duration_secs: Some(3600.0),
            max_upload_bytes: Some(24 * 1024 * 1024),
            max_text_chars: None,
            supports_cancellation: false,
            requires_network: true,
            local_only_ok: false,
            output_formats: vec!["txt".into(), "json".into()],
            notes: vec![
                "LLM-assisted multimodal chat path; timestamps are unreliable.".into(),
                "Do not use SRT when timestamps_reliable is false.".into(),
            ],
        },
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
    // Dedicated /audio/transcriptions ASR
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
    // Multimodal chat (LLM-assisted; timestamps unreliable)
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
///
/// * `Chat` / `Transcriptions` — honour the override for any model id.
/// * `Auto` — only models present in [`OPENROUTER_STT_REGISTRY`]; unknown models
///   fail closed with [`UnsupportedCapability`] (no silent name guessing).
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
    ProviderCapabilities {
        schema_version: CAPABILITY_SCHEMA_VERSION,
        provider: "rules".into(),
        model: "builtin".into(),
        operation: CapabilityOperation::Cleanup,
        stt_backend: None,
        timestamps_reliable: false,
        languages: if english {
            vec!["en".into(), "auto".into()]
        } else {
            vec![lang]
        },
        max_duration_secs: None,
        max_upload_bytes: None,
        max_text_chars: Some(500_000),
        supports_cancellation: true,
        requires_network: false,
        local_only_ok: true,
        output_formats: vec!["txt".into(), "json".into()],
        notes: if english {
            vec!["English filler/contraction heuristics apply for clean/professional.".into()]
        } else {
            vec![
                "Non-English: only whitespace-safe normalization; no English filler deletion."
                    .into(),
            ]
        },
    }
}

/// Local Kitten TTS capabilities.
pub fn local_tts_capabilities(model: &str) -> ProviderCapabilities {
    ProviderCapabilities {
        schema_version: CAPABILITY_SCHEMA_VERSION,
        provider: "local".into(),
        model: model.into(),
        operation: CapabilityOperation::Tts,
        stt_backend: None,
        timestamps_reliable: false,
        languages: vec!["en".into()],
        max_duration_secs: None,
        max_upload_bytes: None,
        max_text_chars: Some(5_000),
        supports_cancellation: true,
        requires_network: false,
        local_only_ok: true,
        output_formats: vec!["wav".into(), "json".into()],
        notes: vec!["English KittenTTS ONNX path only.".into()],
    }
}

/// Preflight: reject offline OpenRouter, non-English TTS, etc.
pub fn preflight_stt(
    provider: &str,
    model: &str,
    want_srt: bool,
    local_only: bool,
    stt_mode: OpenRouterSttMode,
) -> Result<ProviderCapabilities> {
    match provider {
        "local" => {
            let caps = local_whisper_capabilities(model);
            Ok(caps)
        }
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
            // Prefer registry truth for backend/timestamps when the model is known.
            if let Some(rec) = lookup_openrouter_stt(model) {
                caps.stt_backend = Some(rec.backend);
                caps.timestamps_reliable = rec.timestamps_reliable;
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

pub fn preflight_tts(language: &str, local_only: bool) -> Result<ProviderCapabilities> {
    let caps = local_tts_capabilities("kitten-nano-int8");
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
    let _ = local_only;
    Ok(caps)
}

pub fn preflight_cleanup(
    provider: &str,
    language: &str,
    style: &str,
) -> Result<ProviderCapabilities> {
    match provider {
        "rules" | "local" => {
            let caps = rules_cleanup_capabilities(language);
            let style = style.trim().to_ascii_lowercase();
            let lang = language.trim().to_ascii_lowercase();
            let english =
                lang.is_empty() || lang == "auto" || lang == "en" || lang.starts_with("en-");
            if !english
                && matches!(
                    style.as_str(),
                    "clean" | "professional" | "bullets" | "summary"
                )
            {
                // Allowed but degraded — still return caps with notes (honest).
            }
            Ok(caps)
        }
        "openrouter" => Ok(ProviderCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            provider: "openrouter".into(),
            model: "chat".into(),
            operation: CapabilityOperation::Cleanup,
            stt_backend: None,
            timestamps_reliable: false,
            languages: vec!["auto".into()],
            max_duration_secs: None,
            max_upload_bytes: None,
            max_text_chars: Some(100_000),
            supports_cancellation: false,
            requires_network: true,
            local_only_ok: false,
            output_formats: vec!["txt".into(), "json".into()],
            notes: vec!["LLM rewrite; not deterministic.".into()],
        }),
        other => Err(UserError::Other {
            message: format!("unknown cleanup provider '{other}'"),
        }
        .into()),
    }
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
        // Must not silently claim transcriptions or chat via name guessing.
        assert!(lookup_openrouter_stt("acme/totally-unknown-asr-v99").is_none());
    }

    #[test]
    fn auto_does_not_guess_whisper_substring() {
        // A model that merely contains "whisper" but is not registered must fail.
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
    fn capability_json_no_secrets() {
        let caps = local_whisper_capabilities("base");
        let s = serde_json::to_string(&caps).unwrap();
        assert!(!s.contains("sk-"));
        assert!(s.contains("schema_version"));
    }
}
