//! Capability / descriptor conformance hooks (JOE-1936).
//!
//! Deterministic checks that a provider's *declared* capabilities do not
//! contradict its descriptor or (for fakes in tests) its stated network needs.
//! Full behavioral conformance against live inference is out of scope here.

use super::descriptor::{NetworkRequirement, ProviderDescriptor};
use super::id::ProviderId;
use super::registry::ProviderRegistry;
use crate::capabilities::{CapabilityOperation, ProviderCapabilities};
use crate::error::{Result, UserError};
use std::collections::HashSet;
use std::fmt;

/// A single conformance failure (honest, actionable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceFailure {
    pub provider: String,
    pub check: &'static str,
    pub detail: String,
}

impl fmt::Display for ConformanceFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capability conformance failed for '{}': {} — {}",
            self.provider, self.check, self.detail
        )
    }
}

impl From<ConformanceFailure> for crate::error::TranscriptionError {
    fn from(c: ConformanceFailure) -> Self {
        UserError::Other {
            message: c.to_string(),
        }
        .into()
    }
}

/// Descriptor network claim must agree with capability `requires_network` /
/// `local_only_ok` (no lying about offline suitability).
pub fn check_network_claim(
    desc: &ProviderDescriptor,
    caps: &ProviderCapabilities,
) -> std::result::Result<(), ConformanceFailure> {
    match desc.network {
        NetworkRequirement::LocalOnly => {
            if caps.requires_network {
                return Err(ConformanceFailure {
                    provider: desc.id.as_str().into(),
                    check: "network_claim",
                    detail: "descriptor is LocalOnly but capabilities.requires_network is true"
                        .into(),
                });
            }
            if !caps.local_only_ok {
                return Err(ConformanceFailure {
                    provider: desc.id.as_str().into(),
                    check: "network_claim",
                    detail: "descriptor is LocalOnly but capabilities.local_only_ok is false"
                        .into(),
                });
            }
        }
        NetworkRequirement::RequiresNetwork => {
            if !caps.requires_network {
                return Err(ConformanceFailure {
                    provider: desc.id.as_str().into(),
                    check: "network_claim",
                    detail: "descriptor RequiresNetwork but capabilities.requires_network is false"
                        .into(),
                });
            }
            if caps.local_only_ok {
                return Err(ConformanceFailure {
                    provider: desc.id.as_str().into(),
                    check: "network_claim",
                    detail: "descriptor RequiresNetwork but capabilities.local_only_ok is true"
                        .into(),
                });
            }
        }
    }
    Ok(())
}

/// Provider field on capabilities must match the descriptor id.
pub fn check_provider_identity(
    desc: &ProviderDescriptor,
    caps: &ProviderCapabilities,
) -> std::result::Result<(), ConformanceFailure> {
    if caps.provider != desc.id.as_str() {
        return Err(ConformanceFailure {
            provider: desc.id.as_str().into(),
            check: "provider_identity",
            detail: format!(
                "capabilities.provider '{}' != descriptor id '{}'",
                caps.provider,
                desc.id.as_str()
            ),
        });
    }
    Ok(())
}

/// Operation on capabilities must match the factory direction under test.
pub fn check_operation(
    expected: CapabilityOperation,
    caps: &ProviderCapabilities,
) -> std::result::Result<(), ConformanceFailure> {
    if caps.operation != expected {
        return Err(ConformanceFailure {
            provider: caps.provider.clone(),
            check: "operation",
            detail: format!(
                "expected operation {:?}, got {:?}",
                expected, caps.operation
            ),
        });
    }
    Ok(())
}

/// Streaming honesty: Aurum must not claim implementation without advertising.
pub fn check_streaming_honesty(
    caps: &ProviderCapabilities,
) -> std::result::Result<(), ConformanceFailure> {
    if caps.streaming_implemented_by_aurum && !caps.streaming_advertised {
        return Err(ConformanceFailure {
            provider: caps.provider.clone(),
            check: "streaming_honesty",
            detail: "streaming_implemented_by_aurum requires streaming_advertised".into(),
        });
    }
    Ok(())
}

/// Speaking-rate range honesty when rate is supported.
pub fn check_speaking_rate_range(
    caps: &ProviderCapabilities,
) -> std::result::Result<(), ConformanceFailure> {
    if caps.supports_speaking_rate {
        match (caps.speaking_rate_min, caps.speaking_rate_max) {
            (Some(min), Some(max))
                if min > 0.0 && max >= min && f32::is_finite(max) && f32::is_finite(min) =>
            {
                Ok(())
            }
            _ => Err(ConformanceFailure {
                provider: caps.provider.clone(),
                check: "speaking_rate_range",
                detail:
                    "supports_speaking_rate requires finite speaking_rate_min/max with min<=max"
                        .into(),
            }),
        }
    } else {
        Ok(())
    }
}

/// Run the standard static checks for one descriptor + capabilities pair.
pub fn check_descriptor_capabilities(
    desc: &ProviderDescriptor,
    caps: &ProviderCapabilities,
    expected_op: CapabilityOperation,
) -> std::result::Result<(), ConformanceFailure> {
    check_provider_identity(desc, caps)?;
    check_operation(expected_op, caps)?;
    check_network_claim(desc, caps)?;
    check_streaming_honesty(caps)?;
    check_speaking_rate_range(caps)?;
    Ok(())
}

/// Built-in descriptors must have unique (id, direction) identities.
pub fn check_unique_descriptor_identities(registry: &ProviderRegistry) -> Result<()> {
    let mut stt_ids: HashSet<String> = HashSet::new();
    for id in registry.list_stt_ids() {
        if !stt_ids.insert(id.as_str().to_string()) {
            return Err(ConformanceFailure {
                provider: id.as_str().into(),
                check: "unique_stt_id",
                detail: format!("duplicate STT provider id '{id}'"),
            }
            .into());
        }
    }

    #[cfg(feature = "tts")]
    {
        let mut tts_ids: HashSet<String> = HashSet::new();
        for id in registry.list_tts_ids() {
            if !tts_ids.insert(id.as_str().to_string()) {
                return Err(ConformanceFailure {
                    provider: id.as_str().into(),
                    check: "unique_tts_id",
                    detail: format!("duplicate TTS provider id '{id}'"),
                }
                .into());
            }
        }
    }

    let mut seen: HashSet<String> = HashSet::new();
    for d in registry.descriptors() {
        if !seen.insert(d.id.as_str().to_string()) {
            return Err(ConformanceFailure {
                provider: d.id.as_str().into(),
                check: "unique_descriptor_enumeration",
                detail: format!("duplicate descriptor enumeration for '{}'", d.id),
            }
            .into());
        }
    }

    for id in &stt_ids {
        if !seen.contains(id) {
            return Err(ConformanceFailure {
                provider: id.clone(),
                check: "descriptor_coverage",
                detail: format!("STT id '{id}' missing from descriptors()"),
            }
            .into());
        }
    }

    Ok(())
}

/// Run built-in product conformance: unique ids + network claim samples.
pub fn check_builtin_conformance(registry: &ProviderRegistry) -> Result<()> {
    check_unique_descriptor_identities(registry)?;

    for id in registry.list_stt_ids() {
        let factory = registry.stt_factory(&id)?;
        let desc = factory.descriptor();
        let model = sample_model_for(&id, CapabilityOperation::Stt);
        let caps = factory.capabilities(model)?;
        check_descriptor_capabilities(desc, &caps, CapabilityOperation::Stt)?;
    }

    #[cfg(feature = "tts")]
    {
        for id in registry.list_tts_ids() {
            let factory = registry.tts_factory(&id)?;
            let desc = factory.descriptor();
            let model = sample_model_for(&id, CapabilityOperation::Tts);
            let caps = factory.capabilities(model)?;
            check_descriptor_capabilities(desc, &caps, CapabilityOperation::Tts)?;
        }
    }

    Ok(())
}

fn sample_model_for(id: &ProviderId, op: CapabilityOperation) -> &'static str {
    match (id.as_str(), op) {
        ("local", CapabilityOperation::Stt) => "tiny-q5_1",
        ("local", CapabilityOperation::Tts) => "kitten-nano-int8",
        ("openrouter", CapabilityOperation::Stt) => "openai/whisper-large-v3",
        ("openrouter", CapabilityOperation::Tts) => "openai/gpt-4o-mini-tts",
        _ => "default",
    }
}

#[cfg(test)]
mod tests {
    use super::super::descriptor::{ProviderOperations, ProviderStability};
    use super::super::factory::TranscriptionProviderFactory;
    use super::super::registry::ProviderRegistryBuilder;
    use super::*;
    use crate::capabilities::{DescriptorFreshness, SttBackendClass};
    use crate::providers::TranscriptionProvider;
    use std::sync::Arc;

    /// Fake factory that *lies*: descriptor says LocalOnly, caps require network.
    struct LyingNetworkStt {
        desc: ProviderDescriptor,
    }

    impl TranscriptionProviderFactory for LyingNetworkStt {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.desc
        }

        fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
            let mut caps = ProviderCapabilities::with_core(
                self.desc.id.as_str(),
                model,
                CapabilityOperation::Stt,
            );
            caps.stt_backend = Some(SttBackendClass::Asr);
            caps.timestamps_reliable = true;
            caps.languages = vec!["en".into()];
            caps.requires_network = true; // LIE relative to LocalOnly descriptor
            caps.local_only_ok = false;
            caps.output_formats = vec!["txt".into()];
            caps.descriptor_freshness = DescriptorFreshness::Static;
            Ok(caps)
        }

        fn build(
            &self,
            _ctx: &super::super::context::ProviderBuildContext,
        ) -> Result<Arc<dyn TranscriptionProvider>> {
            Err(UserError::Other {
                message: "lying fake has no inference".into(),
            }
            .into())
        }
    }

    #[test]
    fn lying_requires_network_fails_conformance() {
        let desc = ProviderDescriptor::new(
            ProviderId::must("liar"),
            "Liar",
            ProviderOperations::STT_ONLY,
            NetworkRequirement::LocalOnly,
            ProviderStability::TestOnly,
        );
        let factory = LyingNetworkStt { desc };
        let caps = factory.capabilities("x").unwrap();
        let err = check_network_claim(factory.descriptor(), &caps).unwrap_err();
        assert_eq!(err.check, "network_claim");
        assert!(err.detail.contains("requires_network"));
    }

    #[test]
    fn builtin_passes_conformance() {
        let reg = ProviderRegistry::builtin().unwrap();
        check_builtin_conformance(&reg).unwrap();
    }

    #[test]
    fn unique_identities_on_builtin() {
        let reg = ProviderRegistry::builtin().unwrap();
        check_unique_descriptor_identities(&reg).unwrap();
        let ids: Vec<_> = reg.descriptors().iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"local"));
        assert!(ids.contains(&"openrouter"));
        let set: HashSet<_> = ids.iter().copied().collect();
        assert_eq!(set.len(), ids.len());
    }

    #[test]
    fn lying_factory_fails_when_registered_and_checked() {
        let factory: Arc<dyn TranscriptionProviderFactory> = Arc::new(LyingNetworkStt {
            desc: ProviderDescriptor::new(
                ProviderId::must("liar"),
                "Liar",
                ProviderOperations::STT_ONLY,
                NetworkRequirement::LocalOnly,
                ProviderStability::TestOnly,
            ),
        });
        let reg = ProviderRegistryBuilder::default()
            .register_stt(factory)
            .unwrap()
            .build();
        let err = check_builtin_conformance(&reg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("network") || msg.contains("conformance"),
            "{msg}"
        );
    }

    #[test]
    fn streaming_honesty_rejects_implemented_without_advertised() {
        let mut caps = ProviderCapabilities::with_core("x", "m", CapabilityOperation::Tts);
        caps.streaming_implemented_by_aurum = true;
        caps.streaming_advertised = false;
        let err = check_streaming_honesty(&caps).unwrap_err();
        assert_eq!(err.check, "streaming_honesty");
    }
}
