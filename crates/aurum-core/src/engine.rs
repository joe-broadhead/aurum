//! Owned engine boundary for library hosts (JOE-1782 / JOE-1654 / JOE-1784 / JOE-1787 / JOE-1938).
//!
//! # Ownership model
//!
//! [`AurumEngine`] owns:
//! * a [`ValidatedConfig`]
//! * an engine-local [`ResourceGovernor`]
//! * an engine-local [`Metrics`] sink
//! * an engine-local STT context pool ([`SttContextPool`])
//! * an engine-local TTS session pool (when the `tts` feature is enabled)
//! * an immutable [`ProviderRegistry`] (builtin factories by default)
//! * lifecycle bookkeeping for explicit shutdown
//!
//! # Provider resolution (JOE-1938)
//!
//! High-level [`Self::transcribe`] / [`Self::synthesize`] and
//! [`Self::stt_provider`] / [`Self::tts_provider`] route through the registry.
//! Concrete vendor construction stays inside factories; the engine assembles a
//! **single-provider** [`ProviderBuildContext`] (no multi-vendor secret bag).
//!
//! # Isolation (JOE-1784)
//!
//! Engines do **not** share whisper/TTS residency with each other or with the
//! process-global pools used by default `LocalWhisperProvider::new` /
//! `LocalTtsProvider::new`. Shutdown clears **idle** entries in this engine's
//! pools only.
//!
//! Process-global pools remain for CLI and callers that construct providers
//! without an engine. Call [`crate::providers::local::clear_context_cache`] at
//! process exit when using those paths with Metal.

use crate::audio::AudioInput;
use crate::config::{Config, ValidatedConfig};
use crate::doctor::{run_doctor, DoctorReport};
use crate::error::{Result, UserError};
use crate::observability::{Metrics, MetricsSnapshot};
use crate::provider_platform::{
    ProviderBuildContext, ProviderId, ProviderRegistry, ProviderResolveOptions,
};
use crate::providers::local::{LocalWhisperProvider, SttContextPool};
use crate::providers::{
    OpenRouterSttMode, TranscriptionOptions, TranscriptionProvider, TranscriptionResult,
};
use crate::runtime::{GovernorConfig, ResourceGovernor};
use crate::support::{build_support_bundle, SupportBundle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[cfg(feature = "tts")]
use crate::tts::local::{LocalTtsProvider, TtsSessionPool};
#[cfg(feature = "tts")]
use crate::tts::provider::{SynthesisOptions, SynthesisProvider, SynthesisResult};

/// Library-facing engine: validated config + owned governor/metrics/model pools + registry.
pub struct AurumEngine {
    config: ValidatedConfig,
    governor: Arc<ResourceGovernor>,
    metrics: Arc<Metrics>,
    stt_pool: Arc<SttContextPool>,
    #[cfg(feature = "tts")]
    tts_pool: Arc<TtsSessionPool>,
    registry: Arc<ProviderRegistry>,
    closed: AtomicBool,
}

impl std::fmt::Debug for AurumEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("AurumEngine");
        d.field("config", &self.config)
            .field("closed", &self.closed.load(Ordering::SeqCst))
            .field("metrics", &self.metrics.snapshot())
            .field("stt_resident", &self.stt_pool.resident_len())
            .field("registry", &*self.registry);
        #[cfg(feature = "tts")]
        d.field("tts_resident", &self.tts_pool.resident_len());
        d.finish_non_exhaustive()
    }
}

impl AurumEngine {
    /// Build from an already-validated config with default governor settings.
    pub fn new(config: ValidatedConfig) -> Self {
        Self::with_governor(config, GovernorConfig::default())
            .expect("default GovernorConfig is always valid")
    }

    /// Build with an explicit governor profile (mobile/server/custom).
    ///
    /// Validates `gov` before construction (JOE-1917 / F-004).
    pub fn with_governor(config: ValidatedConfig, gov: GovernorConfig) -> Result<Self> {
        let registry = ProviderRegistry::builtin()
            .expect("builtin provider registry must construct (compile-time product factories)");
        Self::with_governor_and_registry(config, gov, registry)
    }

    /// Build with an explicit governor and provider registry (tests / embedders).
    pub fn with_governor_and_registry(
        config: ValidatedConfig,
        gov: GovernorConfig,
        registry: ProviderRegistry,
    ) -> Result<Self> {
        let governor = Arc::new(ResourceGovernor::try_new(gov)?);
        Ok(Self {
            config,
            governor,
            metrics: Arc::new(Metrics::new()),
            stt_pool: Arc::new(SttContextPool::new()),
            #[cfg(feature = "tts")]
            tts_pool: Arc::new(TtsSessionPool::new()),
            registry: Arc::new(registry),
            closed: AtomicBool::new(false),
        })
    }

    /// Load config from the default file/env path and validate.
    pub fn load() -> Result<Self> {
        Ok(Self::new(ValidatedConfig::load()?))
    }

    /// Load from an explicit config file path (must exist).
    pub fn load_from_required(path: &std::path::Path) -> Result<Self> {
        Ok(Self::new(ValidatedConfig::load_from_required(path)?))
    }

    /// Validate a raw [`Config`] and wrap it.
    pub fn from_config(cfg: Config) -> Result<Self> {
        Ok(Self::new(ValidatedConfig::try_from_config(cfg)?))
    }

    pub fn config(&self) -> &Config {
        self.config.as_ref()
    }

    pub fn validated_config(&self) -> &ValidatedConfig {
        &self.config
    }

    pub fn governor(&self) -> &Arc<ResourceGovernor> {
        &self.governor
    }

    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }

    pub fn stt_pool(&self) -> &Arc<SttContextPool> {
        &self.stt_pool
    }

    #[cfg(feature = "tts")]
    pub fn tts_pool(&self) -> &Arc<TtsSessionPool> {
        &self.tts_pool
    }

    /// Immutable provider registry owned by this engine (JOE-1938).
    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn ensure_open(&self) -> Result<()> {
        if self.is_closed() {
            return Err(UserError::Other {
                message: "AurumEngine is closed".into(),
            }
            .into());
        }
        Ok(())
    }

    /// Assemble a single-provider build context from engine config + pools (JOE-1938).
    ///
    /// Secrets are scoped to `id` only via [`Config::provider_secret`].
    pub fn build_context_for(&self, id: &ProviderId) -> Result<ProviderBuildContext> {
        self.build_context_for_with(id, ProviderResolveOptions::default())
    }

    /// Build context with optional CLI/library overrides.
    pub fn build_context_for_with(
        &self,
        id: &ProviderId,
        opts: ProviderResolveOptions,
    ) -> Result<ProviderBuildContext> {
        self.ensure_open()?;
        let cfg = self.config.as_ref();
        let local_only = opts.local_only.unwrap_or(cfg.local_only);
        let stt_mode = match opts.stt_mode {
            Some(m) => m,
            None => OpenRouterSttMode::parse(&cfg.openrouter_stt_mode)?,
        };

        let mut ctx = ProviderBuildContext::new(self.cache_dir().to_path_buf())
            .with_local_only(local_only)
            .with_api_key(cfg.provider_secret(id))
            .with_show_progress(opts.show_progress)
            .with_stt_mode(stt_mode)
            .with_tts_max_chars(Some(cfg.tts_max_chars))
            .with_stt_pool(Arc::clone(&self.stt_pool))
            .with_governor(Arc::clone(&self.governor))
            .with_metrics(Arc::clone(&self.metrics));

        #[cfg(feature = "tts")]
        {
            ctx = ctx.with_tts_pool(Arc::clone(&self.tts_pool));
        }

        // Endpoint knobs only for the selected provider id (no multi-vendor bag).
        match id.as_str() {
            "openrouter" => {
                ctx = ctx
                    .with_base_url(Some(cfg.openrouter_base_url.clone()))
                    .with_allow_custom_endpoint(cfg.openrouter_allow_custom_endpoint)
                    .with_use_system_proxy(cfg.openrouter_use_system_proxy);
            }
            "openai" => {
                if let Some(url) = cfg.providers.openai.base_url.clone() {
                    ctx = ctx.with_base_url(Some(url));
                }
            }
            "elevenlabs" => {
                if let Some(url) = cfg.providers.elevenlabs.base_url.clone() {
                    ctx = ctx.with_base_url(Some(url));
                }
            }
            "xai" => {
                if let Some(url) = cfg.providers.xai.base_url.clone() {
                    ctx = ctx.with_base_url(Some(url));
                }
            }
            _ => {}
        }

        Ok(ctx)
    }

    /// Construct an STT provider via the engine registry (JOE-1938).
    pub fn stt_provider(&self, id: &ProviderId) -> Result<Arc<dyn TranscriptionProvider>> {
        self.stt_provider_with(id, ProviderResolveOptions::default())
    }

    pub fn stt_provider_with(
        &self,
        id: &ProviderId,
        opts: ProviderResolveOptions,
    ) -> Result<Arc<dyn TranscriptionProvider>> {
        self.ensure_open()?;
        let factory = self.registry.stt_factory(id)?;
        let ctx = self.build_context_for_with(id, opts)?;
        factory.build(&ctx)
    }

    /// Construct a TTS provider via the engine registry (JOE-1938).
    #[cfg(feature = "tts")]
    pub fn tts_provider(&self, id: &ProviderId) -> Result<Arc<dyn SynthesisProvider>> {
        self.tts_provider_with(id, ProviderResolveOptions::default())
    }

    #[cfg(feature = "tts")]
    pub fn tts_provider_with(
        &self,
        id: &ProviderId,
        opts: ProviderResolveOptions,
    ) -> Result<Arc<dyn SynthesisProvider>> {
        self.ensure_open()?;
        let factory = self.registry.tts_factory(id)?;
        let ctx = self.build_context_for_with(id, opts)?;
        factory.build(&ctx)
    }

    /// Parse config STT provider string to [`ProviderId`].
    pub fn stt_provider_id(&self) -> Result<ProviderId> {
        ProviderId::parse(&self.config.as_ref().provider)
    }

    /// Parse config TTS provider string to [`ProviderId`].
    #[cfg(feature = "tts")]
    pub fn tts_provider_id(&self) -> Result<ProviderId> {
        ProviderId::parse(&self.config.as_ref().tts_provider)
    }

    /// Local STT provider bound to this engine's pool and governor (JOE-1784).
    ///
    /// Prefer [`Self::stt_provider`] with [`ProviderId::local`] for new code.
    pub fn local_whisper(&self) -> Result<LocalWhisperProvider> {
        self.ensure_open()?;
        Ok(LocalWhisperProvider::with_runtime(
            self.cache_dir().to_path_buf(),
            Arc::clone(&self.stt_pool),
            Arc::clone(&self.governor),
        )
        .with_progress(false))
    }

    /// Local TTS provider bound to this engine's pool and governor (JOE-1784).
    #[cfg(feature = "tts")]
    pub fn local_tts(&self) -> Result<LocalTtsProvider> {
        self.ensure_open()?;
        Ok(LocalTtsProvider::with_runtime(
            self.cache_dir().to_path_buf(),
            Arc::clone(&self.tts_pool),
            Arc::clone(&self.governor),
        )
        .with_progress(false)
        .with_max_chars(self.config.as_ref().tts_max_chars))
    }

    /// High-level STT from a prepared [`AudioInput`] via config `provider` (JOE-1787 / JOE-1938).
    pub async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        self.ensure_open()?;
        self.metrics.record_start();
        let start = Instant::now();
        let id = self.stt_provider_id()?;
        let provider = self.stt_provider(&id)?;
        let out = provider.transcribe(input, options).await;
        match &out {
            Ok(_) => self.metrics.record_complete(start.elapsed()),
            Err(_) => self.metrics.record_failed(),
        }
        out
    }

    /// High-level STT from mono PCM @ whisper sample rate (JOE-1787 / JOE-1938).
    ///
    /// Builds a validated [`AudioInput`] then routes through the registry so
    /// remote providers share the same path as file-based STT.
    pub async fn transcribe_pcm(
        &self,
        samples: &[f32],
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        let input = AudioInput::from_pcm_slice(samples, crate::audio::WHISPER_SAMPLE_RATE)?;
        self.transcribe(&input, options).await
    }

    /// Preload a local STT model into **this** engine's pool.
    pub async fn preload_stt(&self, model: &str) -> Result<std::path::PathBuf> {
        self.ensure_open()?;
        self.local_whisper()?.preload(model).await
    }

    /// High-level TTS synthesis via config `tts_provider` (JOE-1787 / JOE-1938).
    #[cfg(feature = "tts")]
    pub async fn synthesize(
        &self,
        text: &str,
        options: &SynthesisOptions,
    ) -> Result<SynthesisResult> {
        self.ensure_open()?;
        self.metrics.record_start();
        let start = Instant::now();
        let id = self.tts_provider_id()?;
        let provider = self.tts_provider(&id)?;
        let out = provider.synthesize(text, options).await;
        match &out {
            Ok(_) => self.metrics.record_complete(start.elapsed()),
            Err(_) => self.metrics.record_failed(),
        }
        out
    }

    /// Drop idle model residency in **this** engine's pools (JOE-1784).
    pub fn clear_model_caches(&self) {
        self.stt_pool.clear();
        #[cfg(feature = "tts")]
        self.tts_pool.clear();
    }

    /// Mark the engine closed and clear idle model caches.
    ///
    /// Does not touch process-global pools used by non-engine providers.
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.clear_model_caches();
    }

    /// Read-only doctor report using this engine's config.
    pub fn doctor(&self) -> DoctorReport {
        run_doctor(self.config.as_ref())
    }

    /// Privacy-safe support bundle using this engine's config and **engine** metrics.
    pub fn support_bundle(&self, user_notes: Option<String>) -> SupportBundle {
        let mut bundle = build_support_bundle(self.config.as_ref(), user_notes);
        bundle.metrics = self.metrics.snapshot();
        bundle.redaction_notes.push(format!(
            "metrics are engine-local; stt_resident={}{}",
            self.stt_pool.resident_len(),
            {
                #[cfg(feature = "tts")]
                {
                    format!(", tts_resident={}", self.tts_pool.resident_len())
                }
                #[cfg(not(feature = "tts"))]
                {
                    String::new()
                }
            }
        ));
        bundle
    }

    pub fn metrics_snapshot(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Cache directory from validated config.
    pub fn cache_dir(&self) -> &std::path::Path {
        &self.config.as_ref().cache_dir
    }
}

impl Drop for AurumEngine {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        self.clear_model_caches();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_platform::preflight_stt_with_registry;

    #[test]
    fn independent_engines_have_independent_metrics_and_pools() {
        let a = AurumEngine::load().unwrap();
        let b = AurumEngine::load().unwrap();
        a.metrics().record_start();
        a.metrics()
            .record_complete(std::time::Duration::from_millis(1));
        assert_eq!(a.metrics_snapshot().ops_started, 1);
        assert_eq!(b.metrics_snapshot().ops_started, 0);
        assert!(!std::ptr::eq(
            Arc::as_ptr(a.governor()),
            Arc::as_ptr(b.governor())
        ));
        assert!(!std::ptr::eq(
            Arc::as_ptr(a.stt_pool()),
            Arc::as_ptr(b.stt_pool())
        ));
        #[cfg(feature = "tts")]
        assert!(!std::ptr::eq(
            Arc::as_ptr(a.tts_pool()),
            Arc::as_ptr(b.tts_pool())
        ));
        // Process-global default pool is distinct from engine pools.
        let process = crate::providers::local::process_global_stt_pool();
        assert!(!std::ptr::eq(
            Arc::as_ptr(a.stt_pool()),
            Arc::as_ptr(&process)
        ));
    }

    #[test]
    fn shutdown_flags_closed_and_rejects_local_whisper() {
        let e = AurumEngine::load().unwrap();
        assert!(!e.is_closed());
        e.shutdown();
        assert!(e.is_closed());
        assert!(e.local_whisper().is_err());
        assert!(e.stt_provider(&ProviderId::local()).is_err());
    }

    #[test]
    fn doctor_and_support_bundle_work() {
        let e = AurumEngine::load().unwrap();
        let d = e.doctor();
        assert!(!d.checks.is_empty());
        let b = e.support_bundle(None);
        assert_eq!(b.schema_version, crate::support::SUPPORT_BUNDLE_VERSION);
        let json = b.to_json_pretty().unwrap();
        assert!(json.contains("engine-local") || json.contains("stt_resident"));
    }

    #[test]
    fn local_whisper_uses_engine_pool() {
        let e = AurumEngine::load().unwrap();
        let p = e.local_whisper().unwrap();
        assert!(std::ptr::eq(
            Arc::as_ptr(p.pool()),
            Arc::as_ptr(e.stt_pool())
        ));
        assert!(std::ptr::eq(
            Arc::as_ptr(p.governor()),
            Arc::as_ptr(e.governor())
        ));
    }

    #[test]
    fn registry_stt_local_builds() {
        let e = AurumEngine::load().unwrap();
        let p = e.stt_provider(&ProviderId::local()).unwrap();
        assert_eq!(p.name(), "local");
    }

    #[test]
    fn registry_unknown_stt_fails_closed() {
        let e = AurumEngine::load().unwrap();
        // openai may be a config id but has no STT factory until JOE-1940.
        let err = match e.stt_provider(&ProviderId::must("openai")) {
            Ok(_) => panic!("expected unknown STT factory error"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("openai") || err.to_string().contains("provider"));
    }

    #[test]
    fn openrouter_local_only_rejected() {
        let mut cfg = Config::load().unwrap();
        cfg.local_only = true;
        let e = AurumEngine::from_config(cfg).unwrap();
        let err = match e.stt_provider_with(
            &ProviderId::openrouter(),
            ProviderResolveOptions {
                local_only: Some(true),
                ..Default::default()
            },
        ) {
            Ok(_) => panic!("expected local_only rejection"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("local_only")
                || err.to_string().contains("network")
                || err.to_string().contains("remote")
        );
    }

    #[test]
    fn openrouter_missing_key_fails() {
        let mut cfg = Config::load().unwrap();
        cfg.openrouter_api_key = None;
        let e = AurumEngine::from_config(cfg).unwrap();
        let err = match e.stt_provider(&ProviderId::openrouter()) {
            Ok(_) => panic!("expected missing key"),
            Err(e) => e,
        };
        let s = err.to_string().to_ascii_lowercase();
        assert!(
            s.contains("api") || s.contains("key") || s.contains("auth"),
            "unexpected: {s}"
        );
    }

    #[test]
    fn build_context_scopes_secret_to_id() {
        let mut cfg = Config::load().unwrap();
        cfg.openrouter_api_key = Some(crate::secret::SecretString::new("sk-or-test-secret"));
        let e = AurumEngine::from_config(cfg).unwrap();
        let local_ctx = e.build_context_for(&ProviderId::local()).unwrap();
        assert!(!local_ctx.has_api_key());
        let or_ctx = e.build_context_for(&ProviderId::openrouter()).unwrap();
        assert!(or_ctx.has_api_key());
        let dbg = format!("{or_ctx:?}");
        assert!(!dbg.contains("sk-or-test"));
    }

    #[test]
    fn preflight_openrouter_local_only() {
        let e = AurumEngine::load().unwrap();
        let err = preflight_stt_with_registry(
            e.registry(),
            &ProviderId::openrouter(),
            "openai/whisper-large-v3",
            false,
            true,
            OpenRouterSttMode::Auto,
        )
        .unwrap_err();
        assert!(err.to_string().contains("network") || err.to_string().contains("local"));
    }

    #[cfg(feature = "tts")]
    #[test]
    fn registry_tts_local_builds() {
        let e = AurumEngine::load().unwrap();
        let p = e.tts_provider(&ProviderId::local()).unwrap();
        assert_eq!(p.name(), "local");
    }

    #[test]
    fn shutdown_rejects_stt_provider() {
        let e = AurumEngine::load().unwrap();
        e.shutdown();
        assert!(e.stt_provider(&ProviderId::local()).is_err());
        assert!(e.build_context_for(&ProviderId::local()).is_err());
    }
}
