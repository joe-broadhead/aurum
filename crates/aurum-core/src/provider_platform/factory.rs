//! Direction-specific provider factories (JOE-1933).

use super::context::ProviderBuildContext;
use super::descriptor::ProviderDescriptor;
use crate::capabilities::ProviderCapabilities;
use crate::error::Result;
use crate::providers::TranscriptionProvider;
use std::sync::Arc;

#[cfg(feature = "tts")]
use crate::tts::provider::SynthesisProvider;

/// Constructs STT providers from a bounded build context.
pub trait TranscriptionProviderFactory: Send + Sync {
    fn descriptor(&self) -> &ProviderDescriptor;

    /// Capability declaration for `model` (may be static; remote refresh is JOE-1936).
    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities>;

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn TranscriptionProvider>>;
}

/// Constructs TTS providers from a bounded build context.
#[cfg(feature = "tts")]
pub trait SynthesisProviderFactory: Send + Sync {
    fn descriptor(&self) -> &ProviderDescriptor;

    fn capabilities(&self, model: &str) -> Result<ProviderCapabilities>;

    fn build(&self, ctx: &ProviderBuildContext) -> Result<Arc<dyn SynthesisProvider>>;
}
