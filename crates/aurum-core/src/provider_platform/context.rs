//! Provider construction context with scoped secrets (JOE-1933).

use crate::observability::Metrics;
use crate::providers::local::SttContextPool;
use crate::runtime::ResourceGovernor;
use crate::secret::SecretString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(feature = "tts")]
use crate::tts::local::TtsSessionPool;

/// Bounded inputs for constructing a single provider instance.
///
/// # Secret scoping
///
/// At most **one** provider-scoped API key may be present. Factories must not
/// receive a bag of every vendor credential. Callers (engine / CLI) select the
/// key for the factory being built before invoking [`TranscriptionProviderFactory::build`]
/// / [`SynthesisProviderFactory::build`].
#[derive(Clone)]
pub struct ProviderBuildContext {
    cache_dir: PathBuf,
    local_only: bool,
    /// Secret for *this* provider only, if any.
    api_key: Option<SecretString>,
    base_url: Option<String>,
    allow_custom_endpoint: bool,
    use_system_proxy: bool,
    stt_pool: Option<Arc<SttContextPool>>,
    governor: Option<Arc<ResourceGovernor>>,
    metrics: Option<Arc<Metrics>>,
    #[cfg(feature = "tts")]
    tts_pool: Option<Arc<TtsSessionPool>>,
}

impl ProviderBuildContext {
    /// Minimal context: cache root only (process-global pools used by factories if needed).
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            local_only: false,
            api_key: None,
            base_url: None,
            allow_custom_endpoint: false,
            use_system_proxy: false,
            stt_pool: None,
            governor: None,
            metrics: None,
            #[cfg(feature = "tts")]
            tts_pool: None,
        }
    }

    pub fn with_local_only(mut self, local_only: bool) -> Self {
        self.local_only = local_only;
        self
    }

    /// Attach a **single** provider-scoped secret (already selected by the caller).
    pub fn with_api_key(mut self, key: Option<SecretString>) -> Self {
        self.api_key = key;
        self
    }

    pub fn with_base_url(mut self, base_url: Option<String>) -> Self {
        self.base_url = base_url;
        self
    }

    pub fn with_allow_custom_endpoint(mut self, allow: bool) -> Self {
        self.allow_custom_endpoint = allow;
        self
    }

    pub fn with_use_system_proxy(mut self, use_proxy: bool) -> Self {
        self.use_system_proxy = use_proxy;
        self
    }

    pub fn with_stt_pool(mut self, pool: Arc<SttContextPool>) -> Self {
        self.stt_pool = Some(pool);
        self
    }

    pub fn with_governor(mut self, gov: Arc<ResourceGovernor>) -> Self {
        self.governor = Some(gov);
        self
    }

    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    #[cfg(feature = "tts")]
    pub fn with_tts_pool(mut self, pool: Arc<TtsSessionPool>) -> Self {
        self.tts_pool = Some(pool);
        self
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn local_only(&self) -> bool {
        self.local_only
    }

    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    pub fn allow_custom_endpoint(&self) -> bool {
        self.allow_custom_endpoint
    }

    pub fn use_system_proxy(&self) -> bool {
        self.use_system_proxy
    }

    pub fn stt_pool(&self) -> Option<&Arc<SttContextPool>> {
        self.stt_pool.as_ref()
    }

    pub fn governor(&self) -> Option<&Arc<ResourceGovernor>> {
        self.governor.as_ref()
    }

    pub fn metrics(&self) -> Option<&Arc<Metrics>> {
        self.metrics.as_ref()
    }

    #[cfg(feature = "tts")]
    pub fn tts_pool(&self) -> Option<&Arc<TtsSessionPool>> {
        self.tts_pool.as_ref()
    }

    /// Expose the scoped API key material for auth construction only.
    ///
    /// Returns a fresh `String` clone; does not leak via `Debug` on this context.
    pub fn api_key_exposed(&self) -> Option<String> {
        self.api_key.as_ref().map(|s| s.expose().to_string())
    }

    /// Whether a scoped key is present (without revealing it).
    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }
}

impl std::fmt::Debug for ProviderBuildContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderBuildContext")
            .field("cache_dir", &self.cache_dir)
            .field("local_only", &self.local_only)
            .field("api_key", &self.api_key) // SecretString redacts
            .field("base_url", &self.base_url)
            .field("allow_custom_endpoint", &self.allow_custom_endpoint)
            .field("use_system_proxy", &self.use_system_proxy)
            .field("has_stt_pool", &self.stt_pool.is_some())
            .field("has_governor", &self.governor.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::SecretString;

    #[test]
    fn debug_redacts_api_key() {
        let ctx = ProviderBuildContext::new("/tmp/aurum-test")
            .with_api_key(Some(SecretString::new("sk-super-secret-key-value")));
        let dbg = format!("{ctx:?}");
        assert!(!dbg.contains("sk-super-secret"));
        assert!(dbg.contains("api_key"));
    }

    #[test]
    fn scoped_key_is_optional() {
        let ctx = ProviderBuildContext::new("/tmp/x");
        assert!(!ctx.has_api_key());
        assert!(ctx.api_key_exposed().is_none());
    }
}
