//! Versioned TTS adapter contract (JOE-1615).
//!
//! An ONNX path alone is **not** a supported model: every pack must select a
//! known adapter ID that defines tokenizer/G2P, tensors, voices, limits, and
//! output policy.

use crate::error::{Result, UserError};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Manifest schema version accepted by this build.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Built-in adapter identifiers.
pub const ADAPTER_KITTEN_ONNX_V1: &str = "kitten-onnx-v1";
/// Deterministic sine fake for conformance (no ONNX) — proves the contract is
/// not Kitten-specific (JOE-1615/1616).
pub const ADAPTER_FAKE_SINE_V1: &str = "fake-sine-v1";
/// Kokoro scaffold (JOE-1617/1618) — not shipped for synthesis until pins land.
pub const ADAPTER_KOKORO_ONNX_V0: &str = "kokoro-onnx-v0";

/// Trust level for a model pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrustMode {
    /// Built-in catalogue with reviewed pins.
    #[default]
    Builtin,
    /// Caller-supplied exact digests; all artifacts verify.
    Verified,
    /// Explicit opt-in local files; marked unsupported/unverified.
    LocalUnverified,
}

impl TrustMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Verified => "verified",
            Self::LocalUnverified => "local_unverified",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "builtin" | "built-in" | "catalogue" => Ok(Self::Builtin),
            "verified" | "pinned" => Ok(Self::Verified),
            "local_unverified" | "local-unverified" | "unverified" => Ok(Self::LocalUnverified),
            other => Err(UserError::InvalidConfig {
                reason: format!(
                    "unknown trust mode '{other}' (use builtin|verified|local_unverified)"
                ),
            }
            .into()),
        }
    }

    pub fn is_supported_for_product(self) -> bool {
        matches!(self, Self::Builtin | Self::Verified)
    }
}

/// One file role in a model pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestArtifact {
    pub role: String,
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// Versioned model-pack manifest (on disk as `aurum-tts-manifest.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelPackManifest {
    pub schema_version: u32,
    pub adapter_id: String,
    pub adapter_version: u32,
    pub model_id: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub max_phoneme_tokens: usize,
    pub languages: Vec<String>,
    pub license: String,
    pub trust: TrustMode,
    pub artifacts: Vec<ManifestArtifact>,
    #[serde(default)]
    pub voices: Vec<ManifestVoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestVoice {
    pub id: String,
    pub internal_key: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub notes: String,
}

impl ModelPackManifest {
    pub fn validate_schema(&self) -> Result<()> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "unsupported TTS manifest schema_version {} (need {MANIFEST_SCHEMA_VERSION})",
                    self.schema_version
                ),
            }
            .into());
        }
        if self.adapter_id.trim().is_empty() || self.model_id.trim().is_empty() {
            return Err(UserError::InvalidConfig {
                reason: "manifest adapter_id and model_id are required".into(),
            }
            .into());
        }
        if self.sample_rate_hz == 0 || self.channels == 0 {
            return Err(UserError::InvalidConfig {
                reason: "manifest sample_rate_hz and channels must be non-zero".into(),
            }
            .into());
        }
        if self.artifacts.is_empty() {
            return Err(UserError::InvalidConfig {
                reason: "manifest must list at least one artifact".into(),
            }
            .into());
        }
        if matches!(self.trust, TrustMode::Verified) {
            for a in &self.artifacts {
                if a.sha256.as_ref().map(|s| s.len() != 64).unwrap_or(true) {
                    return Err(UserError::InvalidConfig {
                        reason: format!(
                            "verified trust requires sha256 for artifact role '{}'",
                            a.role
                        ),
                    }
                    .into());
                }
                if a.size_bytes.is_none() {
                    return Err(UserError::InvalidConfig {
                        reason: format!(
                            "verified trust requires size_bytes for artifact role '{}'",
                            a.role
                        ),
                    }
                    .into());
                }
            }
        }
        if !known_adapter_id(&self.adapter_id) {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "unknown TTS adapter '{}' — an ONNX file alone is not supported; use a known adapter id",
                    self.adapter_id
                ),
            }
            .into());
        }
        Ok(())
    }

    pub fn artifact(&self, role: &str) -> Option<&ManifestArtifact> {
        self.artifacts.iter().find(|a| a.role == role)
    }

    pub fn load_path(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| UserError::InvalidConfig {
            reason: format!("read manifest {}: {e}", path.display()),
        })?;
        if bytes.len() > 256 * 1024 {
            return Err(UserError::InvalidConfig {
                reason: "manifest exceeds 256 KiB".into(),
            }
            .into());
        }
        let m: Self = serde_json::from_slice(&bytes).map_err(|e| UserError::InvalidConfig {
            reason: format!("parse manifest: {e}"),
        })?;
        m.validate_schema()?;
        Ok(m)
    }
}

pub fn known_adapter_id(id: &str) -> bool {
    matches!(
        id,
        ADAPTER_KITTEN_ONNX_V1 | ADAPTER_FAKE_SINE_V1 | ADAPTER_KOKORO_ONNX_V0
    )
}

/// Static metadata for a registered adapter.
#[derive(Debug, Clone)]
pub struct AdapterDescriptor {
    pub id: &'static str,
    pub version: u32,
    pub description: &'static str,
    pub required_artifact_roles: &'static [&'static str],
    pub sample_rate_hz: u32,
    pub languages: &'static [&'static str],
    pub license_requirement: &'static str,
    /// When false, synthesis is not product-supported (scaffold only).
    pub synthesis_supported: bool,
}

pub fn list_adapters() -> Vec<AdapterDescriptor> {
    vec![
        AdapterDescriptor {
            id: ADAPTER_KITTEN_ONNX_V1,
            version: 1,
            description: "KittenTTS nano ONNX + misaki-rs G2P (default)",
            required_artifact_roles: &["onnx", "voices", "config"],
            sample_rate_hz: 24_000,
            languages: &["en"],
            license_requirement: "Apache-2.0 weights + MIT G2P",
            synthesis_supported: true,
        },
        AdapterDescriptor {
            id: ADAPTER_FAKE_SINE_V1,
            version: 1,
            description: "Deterministic sine adapter for conformance (no ONNX)",
            required_artifact_roles: &["config"],
            sample_rate_hz: 24_000,
            languages: &["en"],
            license_requirement: "n/a (test fixture)",
            synthesis_supported: true,
        },
        AdapterDescriptor {
            id: ADAPTER_KOKORO_ONNX_V0,
            version: 0,
            description: "Kokoro-82M ONNX scaffold (not shipped; see ADR)",
            required_artifact_roles: &["onnx", "voices", "config"],
            sample_rate_hz: 24_000,
            languages: &["en"],
            license_requirement: "Apache-2.0 (upstream); MIT binary path TBD",
            synthesis_supported: false,
        },
    ]
}

pub fn lookup_adapter(id: &str) -> Result<AdapterDescriptor> {
    list_adapters()
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| {
            UserError::InvalidConfig {
                reason: format!("unknown adapter '{id}'"),
            }
            .into()
        })
}

/// Preflight a manifest against its adapter (no synthesis).
pub fn preflight_manifest(manifest: &ModelPackManifest) -> Result<AdapterDescriptor> {
    manifest.validate_schema()?;
    let adapter = lookup_adapter(&manifest.adapter_id)?;
    for role in adapter.required_artifact_roles {
        if manifest.artifact(role).is_none() {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "adapter '{}' requires artifact role '{role}' in the pack manifest",
                    adapter.id
                ),
            }
            .into());
        }
    }
    if manifest.sample_rate_hz != adapter.sample_rate_hz && adapter.synthesis_supported {
        // Soft warning path: still fail closed for supported adapters with mismatch.
        return Err(UserError::InvalidConfig {
            reason: format!(
                "manifest sample_rate_hz {} does not match adapter {} native {}",
                manifest.sample_rate_hz, adapter.id, adapter.sample_rate_hz
            ),
        }
        .into());
    }
    if !adapter.synthesis_supported {
        return Err(UserError::UnsupportedCapability {
            provider: "tts".into(),
            model: manifest.model_id.clone(),
            reason: format!(
                "adapter '{}' is scaffold-only and not enabled for synthesis",
                adapter.id
            ),
            hint: "use kitten-onnx-v1 (built-in) or wait for a shipped Kokoro pack".into(),
        }
        .into());
    }
    Ok(adapter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kitten_manifest() -> ModelPackManifest {
        ModelPackManifest {
            schema_version: 1,
            adapter_id: ADAPTER_KITTEN_ONNX_V1.into(),
            adapter_version: 1,
            model_id: "kitten-nano-int8".into(),
            sample_rate_hz: 24_000,
            channels: 1,
            max_phoneme_tokens: 400,
            languages: vec!["en".into()],
            license: "Apache-2.0".into(),
            trust: TrustMode::Builtin,
            artifacts: vec![
                ManifestArtifact {
                    role: "onnx".into(),
                    filename: "m.onnx".into(),
                    sha256: None,
                    size_bytes: None,
                },
                ManifestArtifact {
                    role: "voices".into(),
                    filename: "voices.npz".into(),
                    sha256: None,
                    size_bytes: None,
                },
                ManifestArtifact {
                    role: "config".into(),
                    filename: "config.json".into(),
                    sha256: None,
                    size_bytes: None,
                },
            ],
            voices: vec![],
            source: None,
            notes: None,
        }
    }

    #[test]
    fn kitten_preflight_ok() {
        preflight_manifest(&kitten_manifest()).unwrap();
    }

    #[test]
    fn raw_onnx_not_enough() {
        let mut m = kitten_manifest();
        m.adapter_id = "mystery".into();
        assert!(m.validate_schema().is_err());
    }

    #[test]
    fn verified_requires_pins() {
        let mut m = kitten_manifest();
        m.trust = TrustMode::Verified;
        assert!(m.validate_schema().is_err());
    }

    #[test]
    fn kokoro_not_for_synthesis() {
        let mut m = kitten_manifest();
        m.adapter_id = ADAPTER_KOKORO_ONNX_V0.into();
        m.adapter_version = 0;
        assert!(preflight_manifest(&m).is_err());
    }
}
