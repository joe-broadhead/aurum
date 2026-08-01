//! Owned engine boundary for library hosts (JOE-1782 / JOE-1654 / JOE-1784 / JOE-1787).
//!
//! # Ownership model
//!
//! [`AurumEngine`] owns:
//! * a [`ValidatedConfig`]
//! * an engine-local [`ResourceGovernor`]
//! * an engine-local [`Metrics`] sink
//! * an engine-local STT context pool ([`SttContextPool`])
//! * an engine-local TTS session pool (when the `tts` feature is enabled)
//! * lifecycle bookkeeping for explicit shutdown
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
use crate::providers::local::{LocalWhisperProvider, SttContextPool};
use crate::providers::{TranscriptionOptions, TranscriptionProvider, TranscriptionResult};
use crate::runtime::{GovernorConfig, ResourceGovernor};
use crate::support::{build_support_bundle, SupportBundle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[cfg(feature = "tts")]
use crate::tts::local::{LocalTtsProvider, TtsSessionPool};
#[cfg(feature = "tts")]
use crate::tts::provider::{SynthesisOptions, SynthesisProvider, SynthesisResult};

/// Library-facing engine: validated config + owned governor/metrics/model pools.
pub struct AurumEngine {
    config: ValidatedConfig,
    governor: Arc<ResourceGovernor>,
    metrics: Arc<Metrics>,
    stt_pool: Arc<SttContextPool>,
    #[cfg(feature = "tts")]
    tts_pool: Arc<TtsSessionPool>,
    closed: AtomicBool,
}

impl std::fmt::Debug for AurumEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("AurumEngine");
        d.field("config", &self.config)
            .field("closed", &self.closed.load(Ordering::SeqCst))
            .field("metrics", &self.metrics.snapshot())
            .field("stt_resident", &self.stt_pool.resident_len());
        #[cfg(feature = "tts")]
        d.field("tts_resident", &self.tts_pool.resident_len());
        d.finish_non_exhaustive()
    }
}

impl AurumEngine {
    /// Build from an already-validated config with default governor settings.
    pub fn new(config: ValidatedConfig) -> Self {
        Self::with_governor(config, GovernorConfig::default())
    }

    /// Build with an explicit governor profile (mobile/server/custom).
    pub fn with_governor(config: ValidatedConfig, gov: GovernorConfig) -> Self {
        Self {
            config,
            governor: Arc::new(ResourceGovernor::new(gov)),
            metrics: Arc::new(Metrics::new()),
            stt_pool: Arc::new(SttContextPool::new()),
            #[cfg(feature = "tts")]
            tts_pool: Arc::new(TtsSessionPool::new()),
            closed: AtomicBool::new(false),
        }
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

    /// Local STT provider bound to this engine's pool and governor (JOE-1784).
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

    /// High-level STT from a prepared [`AudioInput`] (JOE-1787).
    pub async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        self.ensure_open()?;
        self.metrics.record_start();
        let start = Instant::now();
        let provider = self.local_whisper()?.with_progress(false);
        let out = provider.transcribe(input, options).await;
        match &out {
            Ok(_) => self.metrics.record_complete(start.elapsed()),
            Err(_) => self.metrics.record_failed(),
        }
        out
    }

    /// High-level STT from mono PCM @ whisper sample rate (JOE-1787).
    pub async fn transcribe_pcm(
        &self,
        samples: &[f32],
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        self.ensure_open()?;
        self.metrics.record_start();
        let start = Instant::now();
        let provider = self.local_whisper()?.with_progress(false);
        let out = provider.transcribe_pcm(samples, options).await;
        match &out {
            Ok(_) => self.metrics.record_complete(start.elapsed()),
            Err(_) => self.metrics.record_failed(),
        }
        out
    }

    /// Preload a local STT model into **this** engine's pool.
    pub async fn preload_stt(&self, model: &str) -> Result<std::path::PathBuf> {
        self.ensure_open()?;
        self.local_whisper()?.preload(model).await
    }

    /// High-level local TTS synthesis (JOE-1787).
    #[cfg(feature = "tts")]
    pub async fn synthesize(
        &self,
        text: &str,
        options: &SynthesisOptions,
    ) -> Result<SynthesisResult> {
        self.ensure_open()?;
        self.metrics.record_start();
        let start = Instant::now();
        let provider = self.local_tts()?;
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
}
