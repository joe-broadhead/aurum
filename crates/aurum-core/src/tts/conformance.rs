//! Adapter/model-pack conformance suite (JOE-1616).
//!
//! Fast fixture tests run on every PR. Real pinned-model integration stays
//! scheduled/release-only (network + large artifacts).

use super::adapter::{
    list_adapters, preflight_manifest, AdapterDescriptor, ModelPackManifest, TrustMode,
    ADAPTER_FAKE_SINE_V1, ADAPTER_KITTEN_ONNX_V1, ADAPTER_KOKORO_ONNX_V0, MANIFEST_SCHEMA_VERSION,
};
use super::catalogue::{lookup_model, DEFAULT_TTS_MODEL, KOKORO_TTS_MODEL};
use super::pack::{load_pack_dir, verify_pack_artifacts};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Machine-readable conformance report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceReport {
    pub schema_version: u32,
    pub adapter_id: String,
    pub model_id: String,
    pub passed: Vec<String>,
    pub failed: Vec<ConformanceFailure>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceFailure {
    pub check: String,
    pub detail: String,
}

impl ConformanceReport {
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Run the fast conformance subset against a pack directory.
pub fn run_pack_conformance(pack_dir: &Path) -> ConformanceReport {
    let mut report = ConformanceReport {
        schema_version: 1,
        adapter_id: String::new(),
        model_id: String::new(),
        passed: vec![],
        failed: vec![],
        skipped: vec![],
    };

    let (root, manifest) = match load_pack_dir(pack_dir, true) {
        Ok(v) => v,
        Err(e) => {
            // Retry without unverified opt-in distinction already inside load
            report.failed.push(ConformanceFailure {
                check: "load_pack".into(),
                detail: e.to_string(),
            });
            return report;
        }
    };
    report.adapter_id = manifest.adapter_id.clone();
    report.model_id = manifest.model_id.clone();
    report.passed.push("load_pack".into());

    check(
        &mut report,
        "manifest_schema",
        manifest.validate_schema().map_err(|e| e.to_string()),
    );
    check(
        &mut report,
        "adapter_preflight",
        preflight_manifest(&manifest)
            .map(|_| ())
            .map_err(|e| e.to_string()),
    );
    check(
        &mut report,
        "artifact_verify",
        verify_pack_artifacts(&root, &manifest).map_err(|e| e.to_string()),
    );
    check(
        &mut report,
        "languages_nonempty",
        if manifest.languages.is_empty() {
            Err("languages empty".into())
        } else {
            Ok(())
        },
    );
    check(
        &mut report,
        "license_present",
        if manifest.license.trim().is_empty() {
            Err("license empty".into())
        } else {
            Ok(())
        },
    );
    check(
        &mut report,
        "sample_rate",
        if manifest.sample_rate_hz == 0 {
            Err("sample_rate_hz is 0".into())
        } else {
            Ok(())
        },
    );

    // Fake adapter can synthesize a short tone without ONNX.
    if manifest.adapter_id == ADAPTER_FAKE_SINE_V1 {
        match synthesize_fake_sine_ms(50) {
            Ok(pcm) if !pcm.is_empty() && pcm.iter().all(|s| s.is_finite()) => {
                report.passed.push("fake_synth_finite_pcm".into());
            }
            Ok(_) => report.failed.push(ConformanceFailure {
                check: "fake_synth_finite_pcm".into(),
                detail: "empty or non-finite".into(),
            }),
            Err(e) => report.failed.push(ConformanceFailure {
                check: "fake_synth_finite_pcm".into(),
                detail: e,
            }),
        }
    } else {
        report
            .skipped
            .push("real_onnx_synthesis (scheduled/integration only)".into());
    }

    report
}

/// Conformance for built-in Kitten catalogue metadata (no network).
pub fn run_kitten_catalogue_conformance() -> ConformanceReport {
    let mut report = ConformanceReport {
        schema_version: 1,
        adapter_id: ADAPTER_KITTEN_ONNX_V1.into(),
        model_id: DEFAULT_TTS_MODEL.into(),
        passed: vec![],
        failed: vec![],
        skipped: vec![],
    };
    match lookup_model(DEFAULT_TTS_MODEL) {
        Ok(info) => {
            report.passed.push("catalogue_lookup".into());
            if info.adapter != ADAPTER_KITTEN_ONNX_V1 {
                report.failed.push(ConformanceFailure {
                    check: "adapter_id".into(),
                    detail: format!("expected {ADAPTER_KITTEN_ONNX_V1}, got {}", info.adapter),
                });
            } else {
                report.passed.push("adapter_id".into());
            }
            if !info.shipped {
                report.failed.push(ConformanceFailure {
                    check: "shipped".into(),
                    detail: "default model must be shipped".into(),
                });
            } else {
                report.passed.push("shipped".into());
            }
            if info.sample_rate_hz == 0 {
                report.failed.push(ConformanceFailure {
                    check: "sample_rate".into(),
                    detail: "zero".into(),
                });
            } else {
                report.passed.push("sample_rate".into());
            }
            // Pins present
            if info.onnx.sha256.len() == 64 && info.voices.sha256.len() == 64 {
                report.passed.push("pins".into());
            } else {
                report.failed.push(ConformanceFailure {
                    check: "pins".into(),
                    detail: "sha256 length".into(),
                });
            }
            report
                .skipped
                .push("warm_cache_synth (integration; requires model on disk)".into());
        }
        Err(e) => report.failed.push(ConformanceFailure {
            check: "catalogue_lookup".into(),
            detail: e.to_string(),
        }),
    }
    report
}

/// Ensure every registered adapter appears in the suite policy.
pub fn adapter_registry_complete() -> Result<(), String> {
    let ids: Vec<_> = list_adapters().into_iter().map(|a| a.id).collect();
    for need in [
        ADAPTER_KITTEN_ONNX_V1,
        ADAPTER_FAKE_SINE_V1,
        ADAPTER_KOKORO_ONNX_V0,
    ] {
        if !ids.contains(&need) {
            return Err(format!("missing adapter {need}"));
        }
    }
    let kokoro = list_adapters()
        .into_iter()
        .find(|a| a.id == ADAPTER_KOKORO_ONNX_V0)
        .ok_or_else(|| "kokoro adapter missing".to_string())?;
    if !kokoro.synthesis_supported {
        return Err("kokoro-onnx-v0 must support synthesis after JOE-1618".into());
    }
    let info = lookup_model(KOKORO_TTS_MODEL).map_err(|e| e.to_string())?;
    if !info.shipped || info.adapter != ADAPTER_KOKORO_ONNX_V0 {
        return Err("kokoro-82m-int8 catalogue entry must be shipped with kokoro-onnx-v0".into());
    }
    Ok(())
}

fn check(report: &mut ConformanceReport, name: &str, res: std::result::Result<(), String>) {
    match res {
        Ok(()) => report.passed.push(name.into()),
        Err(detail) => report.failed.push(ConformanceFailure {
            check: name.into(),
            detail,
        }),
    }
}

/// Deterministic short mono tone for the fake adapter.
pub fn synthesize_fake_sine_ms(duration_ms: u64) -> std::result::Result<Vec<f32>, String> {
    let sr = 24_000u32;
    let n = (sr as u64 * duration_ms.max(1) / 1000) as usize;
    if n == 0 {
        return Err("zero samples".into());
    }
    let mut pcm = Vec::with_capacity(n);
    let freq = 440.0f32;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.2;
        if !s.is_finite() {
            return Err("non-finite".into());
        }
        pcm.push(s);
    }
    Ok(pcm)
}

/// Build a Kitten-shaped manifest from catalogue pins (for inspect/export).
pub fn kitten_builtin_manifest() -> ModelPackManifest {
    let info = lookup_model(DEFAULT_TTS_MODEL).expect("default model");
    ModelPackManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        adapter_id: ADAPTER_KITTEN_ONNX_V1.into(),
        adapter_version: 1,
        model_id: info.id.into(),
        sample_rate_hz: info.sample_rate_hz,
        channels: 1,
        max_phoneme_tokens: info.max_phoneme_tokens,
        languages: info.languages.iter().map(|s| (*s).to_string()).collect(),
        license: info.license.into(),
        trust: TrustMode::Builtin,
        artifacts: vec![
            super::adapter::ManifestArtifact {
                role: "onnx".into(),
                filename: info.onnx.filename.into(),
                sha256: Some(info.onnx.sha256.into()),
                size_bytes: Some(info.onnx.approx_bytes),
            },
            super::adapter::ManifestArtifact {
                role: "voices".into(),
                filename: info.voices.filename.into(),
                sha256: Some(info.voices.sha256.into()),
                size_bytes: Some(info.voices.approx_bytes),
            },
            super::adapter::ManifestArtifact {
                role: "config".into(),
                filename: info.config.filename.into(),
                sha256: Some(info.config.sha256.into()),
                size_bytes: Some(info.config.approx_bytes),
            },
        ],
        voices: super::catalogue::VOICES
            .iter()
            .filter(|v| v.model == info.id)
            .map(|v| super::adapter::ManifestVoice {
                id: v.id.into(),
                internal_key: v.internal_key.into(),
                language: v.language.into(),
                notes: v.notes.into(),
            })
            .collect(),
        source: Some(format!("huggingface:{}", info.hf_repo)),
        notes: Some(info.notes.into()),
    }
}

pub fn describe_adapter(a: &AdapterDescriptor) -> String {
    format!(
        "{} v{} — {} (synth={}, roles={:?})",
        a.id, a.version, a.description, a.synthesis_supported, a.required_artifact_roles
    )
}

#[cfg(test)]
mod tests {
    use super::super::pack::write_fake_sine_pack;
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fake_pack_conformance_passes() {
        let dir = tempdir().unwrap();
        let pack = dir.path().join("p");
        write_fake_sine_pack(&pack, "fake-1").unwrap();
        let r = run_pack_conformance(&pack);
        assert!(r.ok(), "{:?}", r.failed);
        assert!(r.passed.iter().any(|p| p == "fake_synth_finite_pcm"));
    }

    #[test]
    fn kitten_catalogue_conformance() {
        let r = run_kitten_catalogue_conformance();
        assert!(r.ok(), "{:?}", r.failed);
    }

    #[test]
    fn registry_has_two_synth_adapters() {
        adapter_registry_complete().unwrap();
        let synth: Vec<_> = list_adapters()
            .into_iter()
            .filter(|a| a.synthesis_supported)
            .collect();
        assert!(synth.len() >= 2);
    }

    #[test]
    fn broken_digest_fails_suite() {
        let dir = tempdir().unwrap();
        let pack = dir.path().join("p");
        let mut m = write_fake_sine_pack(&pack, "x").unwrap();
        m.artifacts[0].sha256 = Some("00".repeat(32));
        super::super::pack::write_manifest(&pack, &m).unwrap();
        let r = run_pack_conformance(&pack);
        assert!(!r.ok());
    }
}
