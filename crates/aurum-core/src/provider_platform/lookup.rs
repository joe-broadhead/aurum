//! Registry-driven capability lookup and preflight (JOE-1936).
//!
//! Factories own descriptors; this module is the single place that routes
//! operation/model queries through the registry without a central match table.

use super::descriptor::NetworkRequirement;
use super::id::ProviderId;
use super::registry::ProviderRegistry;
use crate::capabilities::{
    apply_stt_request_gates, openrouter_stt_capabilities, preflight_stt_for, preflight_tts_for,
    resolve_openrouter_stt_path, CapabilityOperation, ProviderCapabilities, UnsupportedCapability,
};
use crate::error::{Result, UserError};
use crate::providers::OpenRouterSttMode;

/// Look up capabilities via registered factories.
///
/// * STT / TTS — routed through the matching factory's `capabilities` method.
/// * Cleanup — not yet factory-registered; fails closed with a clear hint.
///
/// Unknown providers or operation mismatches fail closed (no silent fallback).
pub fn capabilities_for(
    registry: &ProviderRegistry,
    provider_id: &ProviderId,
    operation: CapabilityOperation,
    model: &str,
) -> Result<ProviderCapabilities> {
    match operation {
        CapabilityOperation::Stt => {
            let factory = registry.stt_factory(provider_id)?;
            let mut caps = factory.capabilities(model)?;
            caps.provider = provider_id.as_str().into();
            Ok(caps)
        }
        CapabilityOperation::Tts => {
            #[cfg(feature = "tts")]
            {
                let factory = registry.tts_factory(provider_id)?;
                let mut caps = factory.capabilities(model)?;
                caps.provider = provider_id.as_str().into();
                Ok(caps)
            }
            #[cfg(not(feature = "tts"))]
            {
                let _ = (registry, provider_id, model);
                Err(UserError::Other {
                    message: "TTS support is not compiled into this build (feature `tts`)".into(),
                }
                .into())
            }
        }
        CapabilityOperation::Cleanup => Err(UserError::Other {
            message: format!(
                "cleanup is not registry-factory-backed yet for provider '{provider_id}'; \
                 use preflight_cleanup / preflight_cleanup_for"
            ),
        }
        .into()),
    }
}

/// Registry-backed STT preflight: factory must exist; request gates stay fail-closed.
pub fn preflight_stt_with_registry(
    registry: &ProviderRegistry,
    provider: &ProviderId,
    model: &str,
    want_srt: bool,
    local_only: bool,
    stt_mode: OpenRouterSttMode,
) -> Result<ProviderCapabilities> {
    let factory = registry.stt_factory(provider)?;
    let desc = factory.descriptor();

    if local_only && matches!(desc.network, NetworkRequirement::RequiresNetwork) {
        return Err(UnsupportedCapability {
            provider: provider.as_str().into(),
            model: model.into(),
            reason: "provider requires network access".into(),
            hint: "unset local_only or use a LocalOnly provider".into(),
        }
        .into());
    }

    // OpenRouter needs reviewed path resolution + SRT honesty beyond static factory defaults.
    if provider.as_str() == "openrouter" {
        return preflight_stt_for(provider, model, want_srt, local_only, stt_mode);
    }

    let mut caps = factory.capabilities(model)?;
    caps.provider = provider.as_str().into();
    apply_stt_request_gates(&caps, want_srt, local_only)?;
    let _ = stt_mode;
    Ok(caps)
}

/// Registry-backed TTS preflight.
pub fn preflight_tts_with_registry(
    registry: &ProviderRegistry,
    provider: &ProviderId,
    model: &str,
    language: &str,
    local_only: bool,
) -> Result<ProviderCapabilities> {
    #[cfg(feature = "tts")]
    {
        let factory = registry.tts_factory(provider)?;
        let desc = factory.descriptor();

        if local_only && matches!(desc.network, NetworkRequirement::RequiresNetwork) {
            return Err(UnsupportedCapability {
                provider: provider.as_str().into(),
                model: model.into(),
                reason: "provider requires network access".into(),
                hint: "unset local_only or use a LocalOnly TTS provider".into(),
            }
            .into());
        }

        if provider.as_str() == "local" {
            let mut caps = preflight_tts_for(provider, language, local_only)?;
            if !model.trim().is_empty() {
                caps.model = model.into();
            }
            return Ok(caps);
        }

        let mut caps = factory.capabilities(model)?;
        caps.provider = provider.as_str().into();
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
        if !caps.languages.is_empty()
            && !lang.is_empty()
            && !caps.languages.iter().any(|l| {
                l == "auto"
                    || l == &lang
                    || (l == "en" && (lang == "en" || lang.starts_with("en-")))
            })
        {
            return Err(UnsupportedCapability {
                provider: provider.as_str().into(),
                model: caps.model.clone(),
                reason: format!("TTS language '{language}' is not supported"),
                hint: format!("supported languages: {}", caps.languages.join(", ")),
            }
            .into());
        }
        Ok(caps)
    }
    #[cfg(not(feature = "tts"))]
    {
        let _ = (registry, provider, model, language, local_only);
        Err(UserError::Other {
            message: "TTS support is not compiled into this build (feature `tts`)".into(),
        }
        .into())
    }
}

/// Resolve OpenRouter caps through path registry then factory-shaped output.
#[allow(dead_code)]
pub(crate) fn openrouter_caps_resolved(
    model: &str,
    stt_mode: OpenRouterSttMode,
) -> Result<ProviderCapabilities> {
    let path = resolve_openrouter_stt_path(stt_mode, model)?;
    Ok(openrouter_stt_capabilities(model, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::SttBackendClass;

    #[test]
    fn capabilities_for_local_stt() {
        let reg = ProviderRegistry::builtin().unwrap();
        let caps = capabilities_for(
            &reg,
            &ProviderId::local(),
            CapabilityOperation::Stt,
            "tiny-q5_1",
        )
        .unwrap();
        assert_eq!(caps.provider, "local");
        assert_eq!(caps.operation, CapabilityOperation::Stt);
        assert!(caps.local_only_ok);
        assert!(caps.direct_pcm);
    }

    #[test]
    #[cfg(feature = "tts")]
    fn capabilities_for_local_tts() {
        let reg = ProviderRegistry::builtin().unwrap();
        let caps = capabilities_for(
            &reg,
            &ProviderId::local(),
            CapabilityOperation::Tts,
            "kitten-nano-int8",
        )
        .unwrap();
        assert_eq!(caps.operation, CapabilityOperation::Tts);
        assert!(caps.supports_speaking_rate);
    }

    #[test]
    fn capabilities_for_unknown_provider_fails() {
        let reg = ProviderRegistry::builtin().unwrap();
        let err = capabilities_for(
            &reg,
            &ProviderId::must("openai"),
            CapabilityOperation::Stt,
            "whisper-1",
        )
        .unwrap_err();
        assert!(err.to_string().contains("openai") || err.to_string().contains("Invalid"));
    }

    #[test]
    fn cleanup_not_factory_backed() {
        let reg = ProviderRegistry::builtin().unwrap();
        let err = capabilities_for(
            &reg,
            &ProviderId::local(),
            CapabilityOperation::Cleanup,
            "builtin",
        )
        .unwrap_err();
        assert!(err.to_string().contains("cleanup"));
    }

    #[test]
    fn preflight_registry_openrouter_offline_fails() {
        let reg = ProviderRegistry::builtin().unwrap();
        let err = preflight_stt_with_registry(
            &reg,
            &ProviderId::openrouter(),
            "openai/whisper-large-v3",
            false,
            true,
            OpenRouterSttMode::Transcriptions,
        )
        .unwrap_err();
        assert!(err.to_string().contains("network") || err.to_string().contains("local_only"));
    }

    #[test]
    fn preflight_registry_srt_blocked_for_chat() {
        let reg = ProviderRegistry::builtin().unwrap();
        let err = preflight_stt_with_registry(
            &reg,
            &ProviderId::openrouter(),
            "google/gemini-2.5-flash",
            true,
            false,
            OpenRouterSttMode::Auto,
        )
        .unwrap_err();
        assert!(err.to_string().contains("SRT") || err.to_string().contains("timestamp"));
    }

    #[test]
    fn preflight_registry_local_ok() {
        let reg = ProviderRegistry::builtin().unwrap();
        let caps = preflight_stt_with_registry(
            &reg,
            &ProviderId::local(),
            "base",
            true,
            true,
            OpenRouterSttMode::Auto,
        )
        .unwrap();
        assert_eq!(caps.stt_backend, Some(SttBackendClass::Asr));
        assert!(caps.timestamps_reliable);
    }
}
