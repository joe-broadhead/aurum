//! Intentional common host surface (JOE-2221).
//!
//! ```rust,ignore
//! use aurum_core::prelude::*;
//! ```
//!
//! Prefer this for new library hosts. Advanced modules (pack parsers, registry
//! builders, process-global pools) remain available via explicit paths and are
//! intentionally **not** re-exported here.

pub use crate::cancel::CancelFlag;
pub use crate::config::{ConfigFile, ValidatedConfig};
pub use crate::doctor::{run_doctor, DoctorReport};
pub use crate::dto::{ErrorDto, SttResultDto};
pub use crate::engine::AurumEngine;
pub use crate::error::{AurumError, ErrorCategory, Result};
pub use crate::provider_platform::{ProviderId, ProviderRegistry, ProviderResolveOptions};
pub use crate::providers::{TranscriptionOptions, TranscriptionProvider, TranscriptionResult};
pub use crate::runtime::{OpContext, OpProgress, ProgressSink};
pub use crate::sdk::{
    AurumConfig, CleanupConfig, OperationOptions, ProviderProfiles, RuntimeConfig, SttConfig,
    TranscriptionRequest,
};
pub use crate::secret::SecretString;

#[cfg(feature = "tts")]
pub use crate::sdk::{SynthesisRequest, TtsConfig};
#[cfg(feature = "tts")]
pub use crate::tts::provider::{SynthesisOptions, SynthesisProvider, SynthesisResult};
