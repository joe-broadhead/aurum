//! Validated custom TTS catalogue registration (JOE-1620).

use super::adapter::{lookup_adapter, ModelPackManifest, TrustMode};
use super::pack::{custom_pack_cache_dir, load_pack_dir};
use crate::error::{Result, UserError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Maximum custom catalogue entries.
pub const MAX_CUSTOM_MODELS: usize = 32;

/// Config schema: `[[tts.custom_models]]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomTtsModelEntry {
    pub id: String,
    pub adapter: String,
    /// Local pack directory (preferred for v0.0.3).
    #[serde(default)]
    pub pack_dir: Option<String>,
    pub trust: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Validated in-memory custom model.
#[derive(Debug, Clone)]
pub struct CustomTtsModel {
    pub id: String,
    pub adapter: String,
    pub pack_dir: PathBuf,
    pub trust: TrustMode,
    pub license: String,
    pub notes: String,
    pub manifest: ModelPackManifest,
}

/// Parse and validate custom model entries from config.
pub fn validate_custom_models(entries: &[CustomTtsModelEntry]) -> Result<Vec<CustomTtsModel>> {
    if entries.len() > MAX_CUSTOM_MODELS {
        return Err(UserError::InvalidConfig {
            reason: format!(
                "too many [[tts.custom_models]] entries ({} > {MAX_CUSTOM_MODELS})",
                entries.len()
            ),
        }
        .into());
    }
    let mut out = Vec::with_capacity(entries.len());
    let mut ids = std::collections::HashSet::new();
    for e in entries {
        let id = e.id.trim().to_string();
        if id.is_empty() {
            return Err(UserError::InvalidConfig {
                reason: "custom TTS model id must be non-empty".into(),
            }
            .into());
        }
        // Reserved built-in namespace.
        if id == super::catalogue::DEFAULT_TTS_MODEL
            || id == super::catalogue::PLACEHOLDER_ADAPTER_MODEL
            || super::catalogue::lookup_model(&id).is_ok()
                && super::catalogue::lookup_model(&id)
                    .map(|m| m.shipped)
                    .unwrap_or(false)
        {
            // Only reject if it collides with a shipped built-in id.
            if let Ok(m) = super::catalogue::lookup_model(&id) {
                if m.shipped {
                    return Err(UserError::InvalidConfig {
                        reason: format!(
                            "custom model id '{id}' collides with built-in catalogue entry"
                        ),
                    }
                    .into());
                }
            }
        }
        if !ids.insert(id.clone()) {
            return Err(UserError::InvalidConfig {
                reason: format!("duplicate custom TTS model id '{id}'"),
            }
            .into());
        }
        let _adapter = lookup_adapter(&e.adapter)?;
        let trust = TrustMode::parse(&e.trust)?;
        if matches!(trust, TrustMode::Builtin) {
            return Err(UserError::InvalidConfig {
                reason: "custom models cannot use trust=builtin".into(),
            }
            .into());
        }
        let pack_dir = e.pack_dir.as_ref().ok_or_else(|| UserError::InvalidConfig {
            reason: format!(
                "custom model '{id}' requires pack_dir (remote custom packs are not enabled in v0.0.3)"
            ),
        })?;
        let path = PathBuf::from(pack_dir);
        let allow_unverified = matches!(trust, TrustMode::LocalUnverified);
        let (_root, manifest) = load_pack_dir(&path, allow_unverified)?;
        if manifest.adapter_id != e.adapter {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "custom model '{id}' adapter mismatch: config={} pack={}",
                    e.adapter, manifest.adapter_id
                ),
            }
            .into());
        }
        out.push(CustomTtsModel {
            id,
            adapter: e.adapter.clone(),
            pack_dir: path,
            trust,
            license: e
                .license
                .clone()
                .unwrap_or_else(|| manifest.license.clone()),
            notes: e.notes.clone().unwrap_or_default(),
            manifest,
        });
    }
    Ok(out)
}

/// Status row for listings.
#[derive(Debug, Clone, Serialize)]
pub struct CustomModelStatus {
    pub id: String,
    pub adapter: String,
    pub trust: String,
    pub license: String,
    pub pack_dir: String,
    pub provenance: String,
}

pub fn list_custom_status(models: &[CustomTtsModel]) -> Vec<CustomModelStatus> {
    models
        .iter()
        .map(|m| CustomModelStatus {
            id: m.id.clone(),
            adapter: m.adapter.clone(),
            trust: m.trust.as_str().into(),
            license: m.license.clone(),
            pack_dir: m.pack_dir.display().to_string(),
            provenance: "custom".into(),
        })
        .collect()
}

pub fn format_custom_list(models: &[CustomTtsModel]) -> String {
    if models.is_empty() {
        return "No custom TTS models configured.\n".into();
    }
    let mut out = String::from("Custom TTS models:\n");
    for m in models {
        out.push_str(&format!(
            "  {}  adapter={}  trust={}  license={}\n    pack: {}\n",
            m.id,
            m.adapter,
            m.trust.as_str(),
            m.license,
            m.pack_dir.display()
        ));
    }
    out.push_str("\nBuilt-in models are never shadowed. Custom models are never default.\n");
    let _ = custom_pack_cache_dir(Path::new("."));
    out
}

#[cfg(test)]
mod tests {
    use super::super::pack::write_fake_sine_pack;
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn accepts_valid_custom() {
        let dir = tempdir().unwrap();
        let pack = dir.path().join("p");
        write_fake_sine_pack(&pack, "my-fake").unwrap();
        let entries = vec![CustomTtsModelEntry {
            id: "my-fake".into(),
            adapter: "fake-sine-v1".into(),
            pack_dir: Some(pack.display().to_string()),
            trust: "verified".into(),
            license: Some("CC0".into()),
            notes: None,
        }];
        let v = validate_custom_models(&entries).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn rejects_builtin_collision() {
        let dir = tempdir().unwrap();
        let pack = dir.path().join("p");
        write_fake_sine_pack(&pack, "x").unwrap();
        let entries = vec![CustomTtsModelEntry {
            id: super::super::catalogue::DEFAULT_TTS_MODEL.into(),
            adapter: "fake-sine-v1".into(),
            pack_dir: Some(pack.display().to_string()),
            trust: "verified".into(),
            license: None,
            notes: None,
        }];
        assert!(validate_custom_models(&entries).is_err());
    }
}
