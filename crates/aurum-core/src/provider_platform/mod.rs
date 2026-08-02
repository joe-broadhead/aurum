//! Shared provider platform: identity, registry, factories (JOE-1933 / JOE-1932 / JOE-1936).
//!
//! Direction-specific execution traits remain in [`crate::providers`] (STT) and
//! [`crate::tts::provider`] (TTS). This module owns **how** those implementations
//! are identified, registered, and constructed without flattening STT/TTS
//! semantics. See `docs/development/adr-002-provider-registry.md`.
//!
//! Capability discovery and conformance (JOE-1936) route through the registry
//! via [`capabilities_for`] and [`conformance`].

mod builtin;
mod conformance;
mod context;
mod descriptor;
mod factory;
mod id;
mod listing;
mod lookup;
mod registry;

pub use builtin::{LocalSttFactory, OpenRouterSttFactory};
#[cfg(feature = "tts")]
pub use builtin::{LocalTtsFactory, OpenRouterTtsFactory};
pub use conformance::{
    check_builtin_conformance, check_descriptor_capabilities, check_network_claim,
    check_unique_descriptor_identities, ConformanceFailure,
};
pub use context::ProviderBuildContext;
pub use descriptor::{
    NetworkRequirement, ProviderDescriptor, ProviderOperations, ProviderStability,
};

/// Optional overrides when resolving a provider through [`crate::AurumEngine`] (JOE-1938).
#[derive(Debug, Clone, Default)]
pub struct ProviderResolveOptions {
    /// When true, local STT/TTS may emit progress on stderr.
    pub show_progress: bool,
    /// Override OpenRouter STT mode; default parses config.
    pub stt_mode: Option<crate::providers::OpenRouterSttMode>,
    /// Override `local_only`; default uses config.
    pub local_only: Option<bool>,
}
#[cfg(feature = "tts")]
pub use factory::SynthesisProviderFactory;
pub use factory::TranscriptionProviderFactory;
pub use id::{ProviderId, MAX_PROVIDER_ID_LEN};
pub use listing::{
    list_provider_summaries, merge_provider_summaries, provider_list, ProviderList,
    ProviderSummary, PROVIDER_LIST_SCHEMA_VERSION,
};
pub use lookup::{capabilities_for, preflight_stt_with_registry, preflight_tts_with_registry};
pub use registry::{ProviderRegistry, ProviderRegistryBuilder};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{CapabilityOperation, ProviderCapabilities};
    use crate::secret::SecretString;
    use std::sync::Arc;

    struct FakeStt {
        desc: ProviderDescriptor,
    }

    impl TranscriptionProviderFactory for FakeStt {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.desc
        }

        fn capabilities(&self, model: &str) -> crate::error::Result<ProviderCapabilities> {
            let mut caps = ProviderCapabilities::with_core(
                self.desc.id.as_str(),
                model,
                CapabilityOperation::Stt,
            );
            caps.stt_backend = Some(crate::capabilities::SttBackendClass::Asr);
            caps.timestamps_reliable = true;
            caps.languages = vec!["en".into()];
            caps.max_duration_secs = Some(60.0);
            caps.supports_cancellation = true;
            caps.requires_network = false;
            caps.local_only_ok = true;
            caps.output_formats = vec!["txt".into()];
            Ok(caps)
        }

        fn build(
            &self,
            _ctx: &ProviderBuildContext,
        ) -> crate::error::Result<Arc<dyn crate::providers::TranscriptionProvider>> {
            Err(crate::error::UserError::Other {
                message: "fake stt does not implement inference".into(),
            }
            .into())
        }
    }

    #[test]
    fn rejects_duplicate_stt_registration() {
        let f1: Arc<dyn TranscriptionProviderFactory> = Arc::new(FakeStt {
            desc: ProviderDescriptor::new(
                ProviderId::must("fake"),
                "Fake",
                ProviderOperations::STT_ONLY,
                NetworkRequirement::LocalOnly,
                ProviderStability::TestOnly,
            ),
        });
        let f2: Arc<dyn TranscriptionProviderFactory> = Arc::new(FakeStt {
            desc: ProviderDescriptor::new(
                ProviderId::must("fake"),
                "Fake2",
                ProviderOperations::STT_ONLY,
                NetworkRequirement::LocalOnly,
                ProviderStability::TestOnly,
            ),
        });
        let err = match ProviderRegistry::builder()
            .register_stt(f1)
            .unwrap()
            .register_stt(f2)
        {
            Ok(_) => panic!("expected duplicate registration error"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(msg.contains("duplicate"), "{msg}");
    }

    #[test]
    fn builtin_registers_local_and_openrouter_stt() {
        let reg = ProviderRegistry::builtin().unwrap();
        let local = ProviderId::local();
        let or = ProviderId::openrouter();
        assert!(reg.stt_factory(&local).is_ok());
        assert!(reg.stt_factory(&or).is_ok());
        let caps = reg
            .stt_factory(&local)
            .unwrap()
            .capabilities("tiny-q5_1")
            .unwrap();
        assert_eq!(caps.provider, "local");
        assert!(!caps.requires_network || caps.local_only_ok);

        let unknown = ProviderId::must("openai");
        assert!(reg.stt_factory(&unknown).is_err());
    }

    #[test]
    #[cfg(feature = "tts")]
    fn builtin_registers_local_tts() {
        let reg = ProviderRegistry::builtin().unwrap();
        let local = ProviderId::local();
        assert!(reg.tts_factory(&local).is_ok());
        assert!(reg.tts_factory(&ProviderId::openrouter()).is_err());
    }

    #[test]
    fn descriptors_are_deterministic() {
        let a = ProviderRegistry::builtin().unwrap();
        let b = ProviderRegistry::builtin().unwrap();
        let da: Vec<_> = a.descriptors().iter().map(|d| d.id.as_str()).collect();
        let db: Vec<_> = b.descriptors().iter().map(|d| d.id.as_str()).collect();
        assert_eq!(da, db);
        assert!(da.contains(&"local"));
        assert!(da.contains(&"openrouter"));
    }

    #[test]
    fn openrouter_factory_rejects_local_only() {
        let reg = ProviderRegistry::builtin().unwrap();
        let f = reg.stt_factory(&ProviderId::openrouter()).unwrap();
        let ctx = ProviderBuildContext::new("/tmp/aurum-reg-test")
            .with_local_only(true)
            .with_api_key(Some(SecretString::new("sk-test")));
        let err = match f.build(&ctx) {
            Ok(_) => panic!("expected local_only rejection"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("local_only") || msg.contains("remote"),
            "{msg}"
        );
    }

    #[test]
    fn local_stt_builds_without_secret() {
        let reg = ProviderRegistry::builtin().unwrap();
        let f = reg.stt_factory(&ProviderId::local()).unwrap();
        let ctx = ProviderBuildContext::new(std::env::temp_dir().join("aurum-reg-local"));
        let p = f.build(&ctx).unwrap();
        assert_eq!(p.name(), "local");
    }

    #[test]
    fn secret_scoping_does_not_leak_via_debug() {
        let ctx = ProviderBuildContext::new("/tmp/x")
            .with_api_key(Some(SecretString::new("sk-must-not-appear-in-debug")));
        assert!(!format!("{ctx:?}").contains("sk-must-not-appear"));
    }

    #[test]
    fn capabilities_for_routes_through_registry() {
        let reg = ProviderRegistry::builtin().unwrap();
        let caps =
            capabilities_for(&reg, &ProviderId::local(), CapabilityOperation::Stt, "base").unwrap();
        assert_eq!(caps.provider, "local");
        assert_eq!(
            caps.schema_version,
            crate::capabilities::CAPABILITY_SCHEMA_VERSION
        );
    }
}
