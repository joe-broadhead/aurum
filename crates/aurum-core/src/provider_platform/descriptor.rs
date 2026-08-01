//! Provider descriptors for discovery and preflight (JOE-1933).

use super::id::ProviderId;
use serde::{Deserialize, Serialize};

/// Whether a provider implementation may perform network I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkRequirement {
    /// Never contacts the network for inference (downloads may still be gated separately).
    LocalOnly,
    /// Requires network for the operation.
    RequiresNetwork,
}

/// Stability of a registered provider surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStability {
    /// Supported product path.
    Stable,
    /// Usable but may change without a major version.
    Experimental,
    /// Compiled for tests/fixtures only.
    TestOnly,
}

/// Operations a provider factory may expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderOperations {
    pub stt: bool,
    pub tts: bool,
}

impl ProviderOperations {
    pub const STT_ONLY: Self = Self {
        stt: true,
        tts: false,
    };
    pub const TTS_ONLY: Self = Self {
        stt: false,
        tts: true,
    };
    pub const BOTH: Self = Self {
        stt: true,
        tts: true,
    };

    pub fn supports_stt(self) -> bool {
        self.stt
    }

    pub fn supports_tts(self) -> bool {
        self.tts
    }
}

/// Immutable description of a registered provider (compile-time / registration-time).
#[derive(Debug, Clone)]
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub display_name: &'static str,
    pub operations: ProviderOperations,
    pub network: NetworkRequirement,
    pub stability: ProviderStability,
}

impl ProviderDescriptor {
    pub fn new(
        id: ProviderId,
        display_name: &'static str,
        operations: ProviderOperations,
        network: NetworkRequirement,
        stability: ProviderStability,
    ) -> Self {
        Self {
            id,
            display_name,
            operations,
            network,
            stability,
        }
    }
}
