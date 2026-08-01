//! Built-in factories for local STT/TTS and OpenRouter STT (JOE-1933).

use super::context::ProviderBuildContext;
use super::descriptor::{
    NetworkRequirement, ProviderDescriptor, ProviderOperations, ProviderStability,
};
use super::factory::TranscriptionProviderFactory;
use super::id::ProviderId;
use super::registry::{ProviderRegistry, ProviderRegistryBuilder};
use crate::capabilities::{
    local_whisper_capabilities, openrouter_stt_capabilities, ProviderCapabilities,
};
use crate::error::{Result, UserError};
use crate::providers::local::LocalWhisperProvider;
use crate::providers::openrouter::{OpenRouterProvider, OpenRouterSttMode};
use crate::providers::TranscriptionProvider;
use crate::remote::RemotePolicy;
use std::sync::Arc;

#[cfg(feature = "tts")]
use super::factory::SynthesisProviderFactory;
#[cfg(feature = "tts")]
use crate::capabilities::local_tts_capabilities;
#[cfg(feature = "tts")]
use crate::tts::local::LocalTtsProvider;
#[cfg(feature = "tts")]
use crate::tts::provider::SynthesisProvider;

// ── Local STT ───────────────────────────────────────────────────────────────

pub struct LocalSttFactory {
    descriptor: ProviderDescriptor,
}

impl LocalSttFactory {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::local(),
                "Local Whisper",
                ProviderOperations::STT_ONLY,
                NetworkRequirement::LocalOnly,
                ProviderStability::Stable,
            ),
        }
    }
}

impl Default for LocalSttFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl TranscriptionProviderFactory for LocalSttFactory {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
        Ok(local_whisper_capabilities(model))
    }

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn TranscriptionProvider>> {
        let provider = match (ctx.stt_pool(), ctx.governor()) {
            (Some(pool), Some(gov)) => LocalWhisperProvider::with_runtime(
                ctx.cache_dir().to_path_buf(),
                Arc::clone(pool),
                Arc::clone(gov),
            ),
            _ => LocalWhisperProvider::new(ctx.cache_dir().to_path_buf()),
        }
        .with_progress(false)
        .with_local_only(ctx.local_only());
        Ok(Arc::new(provider))
    }
}

// ── OpenRouter STT ──────────────────────────────────────────────────────────

pub struct OpenRouterSttFactory {
    descriptor: ProviderDescriptor,
}

impl OpenRouterSttFactory {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::openrouter(),
                "OpenRouter",
                ProviderOperations::STT_ONLY,
                NetworkRequirement::RequiresNetwork,
                ProviderStability::Stable,
            ),
        }
    }
}

impl Default for OpenRouterSttFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl TranscriptionProviderFactory for OpenRouterSttFactory {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
        // Default to transcriptions-class declaration; resolve_path may refine at request time.
        use crate::capabilities::OpenRouterSttPath;
        Ok(openrouter_stt_capabilities(
            model,
            OpenRouterSttPath::Transcriptions,
        ))
    }

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn TranscriptionProvider>> {
        if ctx.local_only() {
            return Err(UserError::UnsupportedCapability {
                provider: "openrouter".into(),
                model: "*".into(),
                reason: "remote STT is disabled under local_only".into(),
                hint: "unset local_only or use provider=local".into(),
            }
            .into());
        }
        let key = ctx.api_key_exposed();
        let mut policy = RemotePolicy::default();
        policy.allow_custom_credentialed_endpoint = ctx.allow_custom_endpoint();
        policy.use_system_proxy = ctx.use_system_proxy();
        let provider = OpenRouterProvider::with_policy(
            key,
            ctx.base_url().map(|s| s.to_string()),
            policy,
            OpenRouterSttMode::Auto,
        )?;
        Ok(Arc::new(provider))
    }
}

// ── Local TTS ───────────────────────────────────────────────────────────────

#[cfg(feature = "tts")]
pub struct LocalTtsFactory {
    descriptor: ProviderDescriptor,
}

#[cfg(feature = "tts")]
impl LocalTtsFactory {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::local(),
                "Local TTS",
                ProviderOperations::TTS_ONLY,
                NetworkRequirement::LocalOnly,
                ProviderStability::Stable,
            ),
        }
    }
}

#[cfg(feature = "tts")]
impl Default for LocalTtsFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tts")]
impl SynthesisProviderFactory for LocalTtsFactory {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
        Ok(local_tts_capabilities(model))
    }

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn SynthesisProvider>> {
        let provider = match (ctx.tts_pool(), ctx.governor()) {
            (Some(pool), Some(gov)) => LocalTtsProvider::with_runtime(
                ctx.cache_dir().to_path_buf(),
                Arc::clone(pool),
                Arc::clone(gov),
            ),
            _ => LocalTtsProvider::new(ctx.cache_dir().to_path_buf()),
        }
        .with_progress(false);
        Ok(Arc::new(provider))
    }
}

/// Product built-in registry: local STT, OpenRouter STT, local TTS (when enabled).
pub fn build_builtin_registry() -> Result<ProviderRegistry> {
    let b = ProviderRegistryBuilder::default()
        .register_stt(Arc::new(LocalSttFactory::new()))?
        .register_stt(Arc::new(OpenRouterSttFactory::new()))?;
    #[cfg(feature = "tts")]
    let b = b.register_tts(Arc::new(LocalTtsFactory::new()))?;
    Ok(b.build())
}
