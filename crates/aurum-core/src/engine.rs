//! Owned engine boundary for library hosts (JOE-1782 / JOE-1654).
//!
//! # Ownership model
//!
//! [`AurumEngine`] owns:
//! * a [`ValidatedConfig`]
//! * an engine-local [`ResourceGovernor`]
//! * an engine-local [`Metrics`] sink
//! * lifecycle bookkeeping for explicit shutdown
//!
//! # Residual process-global state (honest)
//!
//! Local whisper `WhisperContext` residency and TTS session pools remain
//! **process-global** in this release line (see `providers::local` and TTS
//! session code). Multiple engines therefore share model caches. Engine
//! shutdown does **not** clear the process whisper cache — call
//! [`crate::providers::local::clear_context_cache`] at process exit when using
//! Metal, as before.
//!
//! Full per-engine model isolation is a follow-up under JOE-1654.

use crate::config::{Config, ValidatedConfig};
use crate::doctor::{run_doctor, DoctorReport};
use crate::error::Result;
use crate::observability::{Metrics, MetricsSnapshot};
use crate::runtime::{GovernorConfig, ResourceGovernor};
use crate::support::{build_support_bundle, SupportBundle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Library-facing engine: validated config + owned governor/metrics.
pub struct AurumEngine {
    config: ValidatedConfig,
    governor: Arc<ResourceGovernor>,
    metrics: Arc<Metrics>,
    closed: AtomicBool,
}

impl std::fmt::Debug for AurumEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AurumEngine")
            .field("config", &self.config)
            .field("closed", &self.closed.load(Ordering::SeqCst))
            .field("metrics", &self.metrics.snapshot())
            .finish_non_exhaustive()
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

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Mark the engine closed for new high-level work.
    ///
    /// Does not clear process-global model caches (see module docs).
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    /// Read-only doctor report using this engine's config.
    pub fn doctor(&self) -> DoctorReport {
        run_doctor(self.config.as_ref())
    }

    /// Privacy-safe support bundle using this engine's config and **engine** metrics.
    pub fn support_bundle(&self, user_notes: Option<String>) -> SupportBundle {
        let mut bundle = build_support_bundle(self.config.as_ref(), user_notes);
        // Prefer engine-local metrics over process-global for multi-engine hosts.
        bundle.metrics = self.metrics.snapshot();
        bundle
            .redaction_notes
            .push("metrics are engine-local (not process-global)".into());
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_engines_have_independent_metrics() {
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
    }

    #[test]
    fn shutdown_flags_closed() {
        let e = AurumEngine::load().unwrap();
        assert!(!e.is_closed());
        e.shutdown();
        assert!(e.is_closed());
    }

    #[test]
    fn doctor_and_support_bundle_work() {
        let e = AurumEngine::load().unwrap();
        let d = e.doctor();
        assert!(!d.checks.is_empty());
        let b = e.support_bundle(None);
        assert_eq!(b.schema_version, crate::support::SUPPORT_BUNDLE_VERSION);
        let json = b.to_json_pretty().unwrap();
        assert!(json.contains("engine-local"));
    }
}
