//! Local TTS model-pack loading, trust modes, and digest verification (JOE-1619).

use super::adapter::{
    lookup_adapter, preflight_manifest, ModelPackManifest, TrustMode, MANIFEST_SCHEMA_VERSION,
};
use crate::error::{EnvironmentError, Result, UserError};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Default manifest filename inside a pack directory.
pub const MANIFEST_FILENAME: &str = "aurum-tts-manifest.json";

/// Maximum single artifact size (512 MiB) for local packs.
pub const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

/// Isolated cache root for custom/local packs (never shadows built-ins).
pub fn custom_pack_cache_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("tts").join("custom")
}

/// Resolve a pack directory → validated manifest + absolute pack root.
pub fn load_pack_dir(
    pack_dir: &Path,
    allow_unverified: bool,
) -> Result<(PathBuf, ModelPackManifest)> {
    let root = canonicalize_local(pack_dir)?;
    let manifest_path = root.join(MANIFEST_FILENAME);
    if !manifest_path.is_file() {
        return Err(UserError::InvalidConfig {
            reason: format!(
                "TTS pack missing {MANIFEST_FILENAME} under {}\n  \
                 Hint: pass a model-pack directory with a manifest, not a bare .onnx file",
                root.display()
            ),
        }
        .into());
    }
    let mut manifest = ModelPackManifest::load_path(&manifest_path)?;
    if matches!(manifest.trust, TrustMode::LocalUnverified) && !allow_unverified {
        return Err(UserError::InvalidConfig {
            reason:
                "local_unverified pack requires explicit opt-in (--allow-unverified / trust mode)"
                    .into(),
        }
        .into());
    }
    // Builtin trust is only for compiled catalogue — local dirs force verified or unverified.
    if matches!(manifest.trust, TrustMode::Builtin) {
        manifest.trust = if allow_unverified {
            TrustMode::LocalUnverified
        } else {
            TrustMode::Verified
        };
    }
    preflight_manifest(&manifest)?;
    verify_pack_artifacts(&root, &manifest)?;
    Ok((root, manifest))
}

fn canonicalize_local(path: &Path) -> Result<PathBuf> {
    let meta = fs::symlink_metadata(path).map_err(|e| UserError::InvalidConfig {
        reason: format!("pack path {}: {e}", path.display()),
    })?;
    if meta.file_type().is_symlink() {
        return Err(UserError::InvalidConfig {
            reason: format!(
                "refusing TTS pack path that is a symlink: {}\n  Hint: pass a real directory",
                path.display()
            ),
        }
        .into());
    }
    if !meta.is_dir() {
        return Err(UserError::InvalidConfig {
            reason: format!(
                "TTS pack path is not a directory: {}\n  Hint: use a pack folder with {MANIFEST_FILENAME}",
                path.display()
            ),
        }
        .into());
    }
    fs::canonicalize(path).map_err(|e| {
        EnvironmentError::DirectoryAccess {
            path: path.display().to_string(),
            reason: e.to_string(),
        }
        .into()
    })
}

/// Verify digests/sizes for pack artifacts relative to `root`.
pub fn verify_pack_artifacts(root: &Path, manifest: &ModelPackManifest) -> Result<()> {
    let adapter = lookup_adapter(&manifest.adapter_id)?;
    for role in adapter.required_artifact_roles {
        let art = manifest
            .artifact(role)
            .ok_or_else(|| UserError::InvalidConfig {
                reason: format!("missing artifact role '{role}'"),
            })?;
        let path = root.join(&art.filename);
        // Reject path escape.
        if art.filename.contains("..") || Path::new(&art.filename).is_absolute() {
            return Err(UserError::InvalidConfig {
                reason: format!("illegal artifact filename '{}'", art.filename),
            }
            .into());
        }
        let meta = fs::metadata(&path).map_err(|e| UserError::InvalidConfig {
            reason: format!("artifact {} missing: {e}", path.display()),
        })?;
        if meta.len() > MAX_ARTIFACT_BYTES {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "artifact {} exceeds max size {MAX_ARTIFACT_BYTES}",
                    art.filename
                ),
            }
            .into());
        }
        if let Some(expect) = art.size_bytes {
            if meta.len() != expect {
                return Err(UserError::InvalidConfig {
                    reason: format!(
                        "artifact {} size mismatch: got {} expected {expect}",
                        art.filename,
                        meta.len()
                    ),
                }
                .into());
            }
        }
        if let Some(expect_hex) = &art.sha256 {
            let got = sha256_file(&path)?;
            if !got.eq_ignore_ascii_case(expect_hex) {
                return Err(UserError::InvalidConfig {
                    reason: format!(
                        "artifact {} sha256 mismatch\n  expected {expect_hex}\n  got      {got}",
                        art.filename
                    ),
                }
                .into());
            }
        } else if matches!(manifest.trust, TrustMode::Verified) {
            return Err(UserError::InvalidConfig {
                reason: format!("verified pack missing sha256 for {}", art.filename),
            }
            .into());
        }
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path).map_err(EnvironmentError::Io)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(EnvironmentError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Write a reviewable manifest into a pack directory (does not download).
pub fn write_manifest(pack_dir: &Path, manifest: &ModelPackManifest) -> Result<PathBuf> {
    manifest.validate_schema()?;
    fs::create_dir_all(pack_dir).map_err(EnvironmentError::Io)?;
    let path = pack_dir.join(MANIFEST_FILENAME);
    let json = serde_json::to_string_pretty(manifest).map_err(|e| {
        crate::error::TranscriptionError::internal(format!("manifest serialize: {e}"))
    })?;
    // Exclusive write via temp rename.
    let tmp = pack_dir.join(format!(".{MANIFEST_FILENAME}.tmp"));
    fs::write(&tmp, json.as_bytes()).map_err(EnvironmentError::Io)?;
    fs::rename(&tmp, &path).map_err(EnvironmentError::Io)?;
    Ok(path)
}

/// Build a minimal fake-sine pack for tests/conformance.
pub fn write_fake_sine_pack(dir: &Path, model_id: &str) -> Result<ModelPackManifest> {
    fs::create_dir_all(dir).map_err(EnvironmentError::Io)?;
    let config = r#"{"adapter":"fake-sine-v1","freq_hz":440}"#;
    let config_path = dir.join("config.json");
    fs::write(&config_path, config).map_err(EnvironmentError::Io)?;
    let sha = sha256_file(&config_path)?;
    let size = fs::metadata(&config_path)
        .map_err(EnvironmentError::Io)?
        .len();
    let manifest = ModelPackManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        adapter_id: super::adapter::ADAPTER_FAKE_SINE_V1.into(),
        adapter_version: 1,
        model_id: model_id.into(),
        sample_rate_hz: 24_000,
        channels: 1,
        max_phoneme_tokens: 256,
        languages: vec!["en".into()],
        license: "CC0-test".into(),
        trust: TrustMode::Verified,
        artifacts: vec![super::adapter::ManifestArtifact {
            role: "config".into(),
            filename: "config.json".into(),
            sha256: Some(sha),
            size_bytes: Some(size),
        }],
        voices: vec![super::adapter::ManifestVoice {
            id: "Tone".into(),
            internal_key: "tone".into(),
            language: "en".into(),
            notes: "440 Hz sine".into(),
        }],
        source: Some("aurum-test-fixture".into()),
        notes: Some("conformance fixture".into()),
    };
    write_manifest(dir, &manifest)?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fake_pack_loads_and_verifies() {
        let dir = tempdir().unwrap();
        let pack = dir.path().join("pack");
        write_fake_sine_pack(&pack, "fake-sine-demo").unwrap();
        let (root, m) = load_pack_dir(&pack, false).unwrap();
        assert_eq!(m.model_id, "fake-sine-demo");
        assert!(root.join("config.json").is_file());
    }

    #[test]
    fn bare_onnx_rejected() {
        let dir = tempdir().unwrap();
        let onnx = dir.path().join("model.onnx");
        fs::write(&onnx, b"not-a-model").unwrap();
        let err = load_pack_dir(dir.path(), true).unwrap_err();
        assert!(err.to_string().contains("manifest") || err.to_string().contains("onnx"));
    }

    #[test]
    fn digest_mismatch_fails() {
        let dir = tempdir().unwrap();
        let pack = dir.path().join("pack");
        let mut m = write_fake_sine_pack(&pack, "x").unwrap();
        m.artifacts[0].sha256 = Some("ab".repeat(32));
        write_manifest(&pack, &m).unwrap();
        assert!(load_pack_dir(&pack, false).is_err());
    }
}
