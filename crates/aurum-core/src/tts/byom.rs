//! TTS BYOM inspect / verify / add helpers (JOE-1621).
//!
//! Never auto-trusts Hugging Face cards or bare ONNX filenames.

use super::adapter::{
    list_adapters, lookup_adapter, preflight_manifest, ManifestArtifact, ModelPackManifest,
    TrustMode, MANIFEST_SCHEMA_VERSION,
};
use super::conformance::{kitten_builtin_manifest, run_pack_conformance, ConformanceReport};
use super::pack::{load_pack_dir, write_manifest, MANIFEST_FILENAME};
use crate::error::{Result, UserError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Read-only inspect result (no network).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectReport {
    pub path: String,
    pub ok: bool,
    pub adapter_id: Option<String>,
    pub model_id: Option<String>,
    pub trust: Option<String>,
    pub languages: Vec<String>,
    pub voices: Vec<String>,
    pub artifacts: Vec<String>,
    pub license: Option<String>,
    pub incompatibilities: Vec<String>,
    pub notes: Vec<String>,
}

/// Inspect a pack directory or manifest file.
pub fn inspect_pack(path: &Path) -> InspectReport {
    let mut report = InspectReport {
        path: path.display().to_string(),
        ok: false,
        adapter_id: None,
        model_id: None,
        trust: None,
        languages: vec![],
        voices: vec![],
        artifacts: vec![],
        license: None,
        incompatibilities: vec![],
        notes: vec![
            "No network access during inspect.".into(),
            "Support is never inferred from filenames alone.".into(),
        ],
    };

    // Bare files that are not the manifest: fail closed without treating the
    // parent directory as a pack (parent may be a symlink like /tmp).
    if path.is_file() {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name != MANIFEST_FILENAME {
            if path.extension().and_then(|e| e.to_str()) == Some("onnx") {
                report.incompatibilities.push(
                    "Bare .onnx files are not supported; provide a pack directory with a manifest and known adapter id."
                        .into(),
                );
            } else {
                report.incompatibilities.push(format!(
                    "expected a pack directory or {MANIFEST_FILENAME}, got file {}",
                    path.display()
                ));
            }
            return report;
        }
    }

    let pack_dir = if path.is_file() {
        path.parent().unwrap_or(path).to_path_buf()
    } else {
        path.to_path_buf()
    };

    match load_pack_dir(&pack_dir, true) {
        Ok((_root, m)) => {
            report.adapter_id = Some(m.adapter_id.clone());
            report.model_id = Some(m.model_id.clone());
            report.trust = Some(m.trust.as_str().into());
            report.languages = m.languages.clone();
            report.voices = m.voices.iter().map(|v| v.id.clone()).collect();
            report.artifacts = m
                .artifacts
                .iter()
                .map(|a| format!("{}:{}", a.role, a.filename))
                .collect();
            report.license = Some(m.license.clone());
            if let Err(e) = preflight_manifest(&m) {
                report.incompatibilities.push(e.to_string());
            }
            if matches!(m.trust, TrustMode::LocalUnverified) {
                report.notes.push(
                    "Trust=local_unverified: unsupported for production; user responsibility."
                        .into(),
                );
            }
            report.ok = report.incompatibilities.is_empty();
        }
        Err(e) => {
            report.incompatibilities.push(e.to_string());
            if path.extension().and_then(|e| e.to_str()) == Some("onnx") {
                report.incompatibilities.push(
                    "Bare .onnx files are not supported; provide a pack directory with a manifest and adapter id."
                        .into(),
                );
            }
        }
    }
    report
}

/// Full verify: load + artifact digests + conformance preflight.
pub fn verify_pack(path: &Path, allow_unverified: bool) -> Result<ConformanceReport> {
    let pack_dir = if path.is_file() {
        path.parent()
            .ok_or_else(|| UserError::InvalidConfig {
                reason: "invalid pack path".into(),
            })?
            .to_path_buf()
    } else {
        path.to_path_buf()
    };
    let _ = load_pack_dir(&pack_dir, allow_unverified)?;
    Ok(run_pack_conformance(&pack_dir))
}

/// Dry-run proposal for `aurum tts add` (no downloads, no config write).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddProposal {
    pub dry_run: bool,
    pub adapter_id: String,
    pub model_id: String,
    pub trust: String,
    pub manifest: ModelPackManifest,
    pub warnings: Vec<String>,
    pub next_steps: Vec<String>,
}

/// Build a proposed manifest for a local pack folder (user must confirm write).
pub fn propose_add_local(
    pack_dir: &Path,
    adapter_id: &str,
    model_id: &str,
    trust: TrustMode,
) -> Result<AddProposal> {
    let adapter = lookup_adapter(adapter_id)?;
    if matches!(trust, TrustMode::Builtin) {
        return Err(UserError::InvalidConfig {
            reason: "cannot register a custom pack as trust=builtin".into(),
        }
        .into());
    }
    // Built-in catalogue ids cannot be shadowed.
    if model_id == super::catalogue::DEFAULT_TTS_MODEL
        || super::catalogue::lookup_model(model_id)
            .map(|m| m.shipped)
            .unwrap_or(false)
    {
        return Err(UserError::InvalidConfig {
            reason: format!(
                "model id '{model_id}' is reserved for the built-in catalogue and cannot be registered as custom"
            ),
        }
        .into());
    }
    // Prefer existing manifest if present.
    let manifest_path = pack_dir.join(MANIFEST_FILENAME);
    let mut warnings = vec![];
    let mut manifest = if manifest_path.is_file() {
        ModelPackManifest::load_path(&manifest_path)?
    } else {
        warnings.push(format!(
            "no {MANIFEST_FILENAME} yet — generating a skeleton with adapter-required roles; \
             fill digests/sizes before using trust=verified"
        ));
        let stub_artifacts: Vec<ManifestArtifact> = adapter
            .required_artifact_roles
            .iter()
            .map(|role| ManifestArtifact {
                role: (*role).into(),
                filename: match *role {
                    "onnx" => "model.onnx".into(),
                    "voices" => "voices.npz".into(),
                    "config" => "config.json".into(),
                    other => format!("{other}.bin"),
                },
                sha256: None,
                size_bytes: None,
            })
            .collect();
        // Skeleton packs cannot claim verified trust without digests.
        let skeleton_trust = if matches!(trust, TrustMode::Verified) {
            warnings.push(
                "requested trust=verified but digests are missing; skeleton uses \
                 local_unverified until pins are filled"
                    .into(),
            );
            TrustMode::LocalUnverified
        } else {
            trust
        };
        ModelPackManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            adapter_id: adapter_id.into(),
            adapter_version: adapter.version,
            model_id: model_id.into(),
            sample_rate_hz: adapter.sample_rate_hz,
            channels: 1,
            max_phoneme_tokens: 400,
            languages: adapter.languages.iter().map(|s| (*s).to_string()).collect(),
            license: "UNKNOWN — required".into(),
            trust: skeleton_trust,
            artifacts: stub_artifacts,
            voices: vec![],
            source: Some(format!("local:{}", pack_dir.display())),
            notes: Some("generated by aurum tts add --dry-run".into()),
        }
    };
    manifest.adapter_id = adapter_id.into();
    manifest.model_id = model_id.into();
    // Keep caller trust only when digests support it.
    if matches!(trust, TrustMode::Verified)
        && manifest
            .artifacts
            .iter()
            .any(|a| a.sha256.as_ref().map(|s| s.len() != 64).unwrap_or(true))
    {
        if !matches!(manifest.trust, TrustMode::LocalUnverified) {
            warnings.push(
                "trust=verified requires sha256+size for every artifact; keeping \
                 local_unverified until pins are complete"
                    .into(),
            );
        }
        manifest.trust = TrustMode::LocalUnverified;
    } else {
        manifest.trust = trust;
    }
    if manifest.license.contains("UNKNOWN") {
        warnings.push("license field must be filled before production use".into());
    }

    Ok(AddProposal {
        dry_run: true,
        adapter_id: adapter_id.into(),
        model_id: model_id.into(),
        trust: manifest.trust.as_str().into(),
        manifest,
        warnings,
        next_steps: vec![
            "Review licenses and digests.".into(),
            "Run: aurum tts verify <pack>".into(),
            "On success, write manifest / add [[tts.custom_models]] in config.".into(),
            "Custom models never become default without an explicit config change.".into(),
        ],
    })
}

/// Materialize a dry-run proposal to disk (manifest only; no network).
pub fn write_add_manifest(pack_dir: &Path, proposal: &AddProposal) -> Result<PathBuf> {
    if proposal.manifest.artifacts.is_empty() {
        return Err(UserError::InvalidConfig {
            reason: "cannot write manifest with zero artifacts; fill adapter roles first".into(),
        }
        .into());
    }
    if matches!(proposal.manifest.trust, TrustMode::Verified)
        && proposal
            .manifest
            .artifacts
            .iter()
            .any(|a| a.sha256.as_ref().map(|s| s.len() != 64).unwrap_or(true))
    {
        return Err(UserError::InvalidConfig {
            reason: "cannot write trust=verified manifest without sha256 for every artifact\n  \
                     Hint: fill digests, or use --trust local_unverified"
                .into(),
        }
        .into());
    }
    write_manifest(pack_dir, &proposal.manifest)
}

/// Human-readable inspect formatting.
pub fn format_inspect(r: &InspectReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("path: {}\n", r.path));
    out.push_str(&format!("ok: {}\n", r.ok));
    if let Some(a) = &r.adapter_id {
        out.push_str(&format!("adapter: {a}\n"));
    }
    if let Some(m) = &r.model_id {
        out.push_str(&format!("model: {m}\n"));
    }
    if let Some(t) = &r.trust {
        out.push_str(&format!("trust: {t}\n"));
    }
    if let Some(l) = &r.license {
        out.push_str(&format!("license: {l}\n"));
    }
    if !r.languages.is_empty() {
        out.push_str(&format!("languages: {}\n", r.languages.join(", ")));
    }
    if !r.voices.is_empty() {
        out.push_str(&format!("voices: {}\n", r.voices.join(", ")));
    }
    if !r.artifacts.is_empty() {
        out.push_str("artifacts:\n");
        for a in &r.artifacts {
            out.push_str(&format!("  - {a}\n"));
        }
    }
    if !r.incompatibilities.is_empty() {
        out.push_str("incompatibilities:\n");
        for i in &r.incompatibilities {
            out.push_str(&format!("  - {i}\n"));
        }
    }
    for n in &r.notes {
        out.push_str(&format!("note: {n}\n"));
    }
    out
}

pub fn format_adapters() -> String {
    let mut out = String::from("Supported TTS adapters:\n");
    for a in list_adapters() {
        out.push_str(&format!(
            "  {} v{}  synth={}  {}\n",
            a.id, a.version, a.synthesis_supported, a.description
        ));
    }
    out.push_str("\nDefault synthesis adapter: kitten-onnx-v1 (built-in catalogue).\n");
    out.push_str("Bare ONNX paths are not supported — use a pack + manifest.\n");
    out
}

/// Export builtin Kitten manifest JSON for docs/examples.
pub fn builtin_kitten_manifest_json() -> Result<String> {
    let m = kitten_builtin_manifest();
    serde_json::to_string_pretty(&m)
        .map_err(|e| crate::error::TranscriptionError::internal(format!("manifest json: {e}")))
}

#[cfg(test)]
mod tests {
    use super::super::pack::write_fake_sine_pack;
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn inspect_fake_pack() {
        let dir = tempdir().unwrap();
        let pack = dir.path().join("p");
        write_fake_sine_pack(&pack, "fake").unwrap();
        let r = inspect_pack(&pack);
        assert!(r.ok, "{:?}", r.incompatibilities);
        assert_eq!(r.adapter_id.as_deref(), Some("fake-sine-v1"));
    }

    #[test]
    fn add_rejects_reserved_id() {
        let dir = tempdir().unwrap();
        let err = propose_add_local(
            dir.path(),
            "fake-sine-v1",
            super::super::catalogue::DEFAULT_TTS_MODEL,
            TrustMode::Verified,
        )
        .unwrap_err();
        assert!(err.to_string().contains("reserved"));
    }

    #[test]
    fn bare_onnx_inspect_fails_closed() {
        let dir = tempdir().unwrap();
        let onnx = dir.path().join("x.onnx");
        std::fs::write(&onnx, b"x").unwrap();
        let r = inspect_pack(&onnx);
        assert!(!r.ok);
    }
}
