//! Built-in factories for local STT/TTS and OpenRouter STT (JOE-1933).

use super::context::ProviderBuildContext;
use super::descriptor::{
    NetworkRequirement, ProviderDescriptor, ProviderOperations, ProviderStability,
};
use super::factory::TranscriptionProviderFactory;
use super::id::ProviderId;
use super::registry::{ProviderRegistry, ProviderRegistryBuilder};
use crate::capabilities::{
    local_whisper_capabilities, ProviderCapabilities,
};
use crate::error::{Result, UserError};
use crate::providers::local::LocalWhisperProvider;
use crate::providers::openrouter::OpenRouterProvider;
use crate::providers::TranscriptionProvider;
use crate::remote::RemotePolicy;
use crate::runtime::ResourceGovernor;
use std::sync::Arc;

#[cfg(feature = "tts")]
use super::factory::SynthesisProviderFactory;
use crate::capabilities::openai_stt_capabilities;
use crate::capabilities::xai_stt_capabilities;
#[cfg(feature = "tts")]
use crate::capabilities::{
    elevenlabs_tts_capabilities, local_tts_capabilities, openai_tts_capabilities,
    openrouter_tts_capabilities, xai_tts_capabilities,
};
#[cfg(feature = "tts")]
use crate::providers::elevenlabs_tts::ElevenLabsTtsProvider;
use crate::providers::openai_stt::OpenAiSttProvider;
#[cfg(feature = "tts")]
use crate::providers::openai_tts::OpenAiTtsProvider;
#[cfg(feature = "tts")]
use crate::providers::openrouter_tts::OpenRouterTtsProvider;
use crate::providers::xai_stt::XaiSttProvider;
#[cfg(feature = "tts")]
use crate::providers::xai_tts::XaiTtsProvider;
#[cfg(feature = "tts")]
use crate::tts::local::LocalTtsProvider;
#[cfg(feature = "tts")]
use crate::tts::provider::SynthesisProvider;

fn engine_or_global_governor(ctx: &ProviderBuildContext) -> Arc<ResourceGovernor> {
    ctx.governor()
        .cloned()
        .unwrap_or_else(ResourceGovernor::process_global)
}

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
        .with_progress(ctx.show_progress())
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
        // Route-aware capability (JOE-1979): Auto resolves reviewed models to the
        // same path as execution. Unknown models fail closed (no false ASR claim).
        use crate::capabilities::{openrouter_stt_capabilities, resolve_openrouter_stt_path};
        use crate::providers::OpenRouterSttMode;
        let path = resolve_openrouter_stt_path(OpenRouterSttMode::Auto, model)?;
        Ok(openrouter_stt_capabilities(model, path))
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
        let key = ctx.api_key_cloned();
        let policy = RemotePolicy {
            allow_custom_credentialed_endpoint: ctx.allow_custom_endpoint(),
            use_system_proxy: ctx.use_system_proxy(),
            allow_loopback_http: ctx
                .base_url()
                .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost")),
            ..RemotePolicy::default()
        };
        let provider = OpenRouterProvider::with_policy(
            key,
            ctx.base_url().map(|s| s.to_string()),
            policy,
            ctx.stt_mode(),
        )?
        .with_governor(engine_or_global_governor(ctx));
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
        let mut provider = match (ctx.tts_pool(), ctx.governor()) {
            (Some(pool), Some(gov)) => LocalTtsProvider::with_runtime(
                ctx.cache_dir().to_path_buf(),
                Arc::clone(pool),
                Arc::clone(gov),
            ),
            _ => LocalTtsProvider::new(ctx.cache_dir().to_path_buf()),
        }
        .with_progress(ctx.show_progress())
        .with_local_only(ctx.local_only());
        if let Some(n) = ctx.tts_max_chars() {
            provider = provider.with_max_chars(n);
        }
        Ok(Arc::new(provider))
    }
}

// ── OpenRouter TTS ──────────────────────────────────────────────────────────

#[cfg(feature = "tts")]
pub struct OpenRouterTtsFactory {
    descriptor: ProviderDescriptor,
}

#[cfg(feature = "tts")]
impl OpenRouterTtsFactory {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::openrouter(),
                "OpenRouter TTS",
                ProviderOperations::TTS_ONLY,
                NetworkRequirement::RequiresNetwork,
                ProviderStability::Stable,
            ),
        }
    }
}

#[cfg(feature = "tts")]
impl Default for OpenRouterTtsFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tts")]
impl SynthesisProviderFactory for OpenRouterTtsFactory {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
        openrouter_tts_capabilities(model)
    }

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn SynthesisProvider>> {
        if ctx.local_only() {
            return Err(UserError::UnsupportedCapability {
                provider: "openrouter".into(),
                model: "*".into(),
                reason: "remote TTS is disabled under local_only".into(),
                hint: "unset local_only or use provider=local".into(),
            }
            .into());
        }
        let key = ctx.api_key_cloned();
        let policy = RemotePolicy {
            allow_custom_credentialed_endpoint: ctx.allow_custom_endpoint(),
            use_system_proxy: ctx.use_system_proxy(),
            allow_loopback_http: ctx
                .base_url()
                .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost")),
            ..RemotePolicy::default()
        };
        let provider =
            OpenRouterTtsProvider::with_policy(key, ctx.base_url().map(|s| s.to_string()), policy)?
                .with_governor(engine_or_global_governor(ctx));
        Ok(Arc::new(provider))
    }
}

// ── OpenAI STT ──────────────────────────────────────────────────────────────

pub struct OpenAiSttFactory {
    descriptor: ProviderDescriptor,
}

impl OpenAiSttFactory {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::must("openai"),
                "OpenAI STT",
                ProviderOperations::STT_ONLY,
                NetworkRequirement::RequiresNetwork,
                ProviderStability::Stable,
            ),
        }
    }
}

impl Default for OpenAiSttFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl TranscriptionProviderFactory for OpenAiSttFactory {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
        openai_stt_capabilities(model)
    }

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn TranscriptionProvider>> {
        if ctx.local_only() {
            return Err(UserError::UnsupportedCapability {
                provider: "openai".into(),
                model: "*".into(),
                reason: "remote STT is disabled under local_only".into(),
                hint: "unset local_only or use provider=local".into(),
            }
            .into());
        }
        let key = ctx.api_key_cloned();
        let policy = RemotePolicy {
            allow_custom_credentialed_endpoint: ctx.allow_custom_endpoint(),
            use_system_proxy: ctx.use_system_proxy(),
            allow_loopback_http: ctx
                .base_url()
                .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost")),
            ..RemotePolicy::default()
        };
        let provider =
            OpenAiSttProvider::with_policy(key, ctx.base_url().map(|s| s.to_string()), policy)?
                .with_governor(engine_or_global_governor(ctx));
        Ok(Arc::new(provider))
    }
}

// ── OpenAI TTS ──────────────────────────────────────────────────────────────

#[cfg(feature = "tts")]
pub struct OpenAiTtsFactory {
    descriptor: ProviderDescriptor,
}

#[cfg(feature = "tts")]
impl OpenAiTtsFactory {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::must("openai"),
                "OpenAI TTS",
                ProviderOperations::TTS_ONLY,
                NetworkRequirement::RequiresNetwork,
                ProviderStability::Stable,
            ),
        }
    }
}

#[cfg(feature = "tts")]
impl Default for OpenAiTtsFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tts")]
impl SynthesisProviderFactory for OpenAiTtsFactory {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
        openai_tts_capabilities(model)
    }

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn SynthesisProvider>> {
        if ctx.local_only() {
            return Err(UserError::UnsupportedCapability {
                provider: "openai".into(),
                model: "*".into(),
                reason: "remote TTS is disabled under local_only".into(),
                hint: "unset local_only or use provider=local".into(),
            }
            .into());
        }
        let key = ctx.api_key_cloned();
        let policy = RemotePolicy {
            allow_custom_credentialed_endpoint: ctx.allow_custom_endpoint(),
            use_system_proxy: ctx.use_system_proxy(),
            allow_loopback_http: ctx
                .base_url()
                .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost")),
            ..RemotePolicy::default()
        };
        let provider =
            OpenAiTtsProvider::with_policy(key, ctx.base_url().map(|s| s.to_string()), policy)?
                .with_governor(engine_or_global_governor(ctx));
        Ok(Arc::new(provider))
    }
}

// ── ElevenLabs TTS ──────────────────────────────────────────────────────────

#[cfg(feature = "tts")]
pub struct ElevenLabsTtsFactory {
    descriptor: ProviderDescriptor,
}

#[cfg(feature = "tts")]
impl ElevenLabsTtsFactory {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::must("elevenlabs"),
                "ElevenLabs TTS",
                ProviderOperations::TTS_ONLY,
                NetworkRequirement::RequiresNetwork,
                ProviderStability::Stable,
            ),
        }
    }
}

#[cfg(feature = "tts")]
impl Default for ElevenLabsTtsFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tts")]
impl SynthesisProviderFactory for ElevenLabsTtsFactory {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
        elevenlabs_tts_capabilities(model)
    }

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn SynthesisProvider>> {
        if ctx.local_only() {
            return Err(UserError::UnsupportedCapability {
                provider: "elevenlabs".into(),
                model: "*".into(),
                reason: "remote TTS is disabled under local_only".into(),
                hint: "unset local_only or use provider=local".into(),
            }
            .into());
        }
        let key = ctx.api_key_cloned();
        let policy = RemotePolicy {
            allow_custom_credentialed_endpoint: ctx.allow_custom_endpoint(),
            use_system_proxy: ctx.use_system_proxy(),
            allow_loopback_http: ctx
                .base_url()
                .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost")),
            ..RemotePolicy::default()
        };
        let provider =
            ElevenLabsTtsProvider::with_policy(key, ctx.base_url().map(|s| s.to_string()), policy)?
                .with_governor(engine_or_global_governor(ctx));
        Ok(Arc::new(provider))
    }
}

// ── xAI STT / TTS ───────────────────────────────────────────────────────────

pub struct XaiSttFactory {
    descriptor: ProviderDescriptor,
}

impl XaiSttFactory {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::must("xai"),
                "xAI STT",
                ProviderOperations::STT_ONLY,
                NetworkRequirement::RequiresNetwork,
                ProviderStability::Experimental,
            ),
        }
    }
}

impl Default for XaiSttFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl TranscriptionProviderFactory for XaiSttFactory {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
        xai_stt_capabilities(model)
    }

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn TranscriptionProvider>> {
        if ctx.local_only() {
            return Err(UserError::UnsupportedCapability {
                provider: "xai".into(),
                model: "*".into(),
                reason: "remote STT is disabled under local_only".into(),
                hint: "unset local_only or use provider=local".into(),
            }
            .into());
        }
        let key = ctx.api_key_cloned();
        let policy = RemotePolicy {
            allow_custom_credentialed_endpoint: ctx.allow_custom_endpoint(),
            use_system_proxy: ctx.use_system_proxy(),
            allow_loopback_http: ctx
                .base_url()
                .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost")),
            ..RemotePolicy::default()
        };
        let provider =
            XaiSttProvider::with_policy(key, ctx.base_url().map(|s| s.to_string()), policy)?
                .with_governor(engine_or_global_governor(ctx));
        Ok(Arc::new(provider))
    }
}

#[cfg(feature = "tts")]
pub struct XaiTtsFactory {
    descriptor: ProviderDescriptor,
}

#[cfg(feature = "tts")]
impl XaiTtsFactory {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor::new(
                ProviderId::must("xai"),
                "xAI TTS",
                ProviderOperations::TTS_ONLY,
                NetworkRequirement::RequiresNetwork,
                ProviderStability::Experimental,
            ),
        }
    }
}

#[cfg(feature = "tts")]
impl Default for XaiTtsFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "tts")]
impl SynthesisProviderFactory for XaiTtsFactory {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities> {
        xai_tts_capabilities(model)
    }

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn SynthesisProvider>> {
        if ctx.local_only() {
            return Err(UserError::UnsupportedCapability {
                provider: "xai".into(),
                model: "*".into(),
                reason: "remote TTS is disabled under local_only".into(),
                hint: "unset local_only or use provider=local".into(),
            }
            .into());
        }
        let key = ctx.api_key_cloned();
        let policy = RemotePolicy {
            allow_custom_credentialed_endpoint: ctx.allow_custom_endpoint(),
            use_system_proxy: ctx.use_system_proxy(),
            allow_loopback_http: ctx
                .base_url()
                .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost")),
            ..RemotePolicy::default()
        };
        let provider =
            XaiTtsProvider::with_policy(key, ctx.base_url().map(|s| s.to_string()), policy)?
                .with_governor(engine_or_global_governor(ctx));
        Ok(Arc::new(provider))
    }
}

/// Product built-in registry: local, OpenRouter, OpenAI, ElevenLabs, xAI.
pub fn build_builtin_registry() -> Result<ProviderRegistry> {
    let b = ProviderRegistryBuilder::default()
        .register_stt(Arc::new(LocalSttFactory::new()))?
        .register_stt(Arc::new(OpenRouterSttFactory::new()))?
        .register_stt(Arc::new(OpenAiSttFactory::new()))?
        .register_stt(Arc::new(XaiSttFactory::new()))?;
    #[cfg(feature = "tts")]
    let b = b
        .register_tts(Arc::new(LocalTtsFactory::new()))?
        .register_tts(Arc::new(OpenRouterTtsFactory::new()))?
        .register_tts(Arc::new(OpenAiTtsFactory::new()))?
        .register_tts(Arc::new(ElevenLabsTtsFactory::new()))?
        .register_tts(Arc::new(XaiTtsFactory::new()))?;
    Ok(b.build())
}
