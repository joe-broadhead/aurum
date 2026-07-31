//! Cache inventory, verification, quarantine, and repair (JOE-1592).

use crate::error::{EnvironmentError, Result, UserError};
use crate::model::{self, ModelInfo, ARTIFACT_MANIFEST_VERSION};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Verification state for a cached artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyState {
    Healthy,
    Missing,
    SizeMismatch,
    DigestMismatch,
    Unpinned,
    Legacy,
    Quarantined,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheEntry {
    pub kind: &'static str,
    pub id: String,
    pub path: String,
    pub manifest_version: &'static str,
    pub expected_bytes: Option<u64>,
    pub actual_bytes: Option<u64>,
    pub expected_sha256: Option<String>,
    pub state: VerifyState,
    pub last_error: Option<String>,
}

/// Cheap status (existence + size) without full hashing.
pub fn status_stt(cache_dir: &Path) -> Vec<CacheEntry> {
    model::list_models(cache_dir)
        .into_iter()
        .map(|row| {
            let expected = model_exact_or_approx(row.info);
            let pin = model::pinned_sha256(row.info.filename);
            let (actual, state) = match fs::metadata(&row.path) {
                Ok(m) if m.len() > 1_000_000 => {
                    let actual = m.len();
                    // Cheap status never claims Healthy (requires digest verify) — JOE-1645.
                    let state = if pin.is_none() {
                        VerifyState::Unpinned
                    } else if let Some(exp) = model::pinned_exact_bytes(row.info.filename) {
                        if actual == exp {
                            // Present + size OK; full integrity is `aurum cache verify`.
                            VerifyState::Legacy
                        } else {
                            VerifyState::SizeMismatch
                        }
                    } else {
                        VerifyState::Legacy
                    };
                    (Some(actual), state)
                }
                Ok(m) if m.len() > 0 => (Some(m.len()), VerifyState::Legacy),
                _ => (None, VerifyState::Missing),
            };
            CacheEntry {
                kind: "stt",
                id: row.info.name.to_string(),
                path: row.path.display().to_string(),
                manifest_version: ARTIFACT_MANIFEST_VERSION,
                expected_bytes: expected,
                actual_bytes: actual,
                expected_sha256: pin.map(|s| s.to_string()),
                state,
                last_error: None,
            }
        })
        .collect()
}

/// Full verify with SHA-256 when pins exist (no network).
/// Invalid on-disk artifacts are moved to quarantine (not deleted).
pub fn verify_stt(cache_dir: &Path) -> Vec<CacheEntry> {
    model::list_models(cache_dir)
        .into_iter()
        .map(|row| verify_one_stt(cache_dir, row.info, &row.path))
        .collect()
}

fn verify_one_stt(cache_dir: &Path, info: &ModelInfo, path: &Path) -> CacheEntry {
    let qdir = quarantine_dir(cache_dir);
    let qpath = qdir.join(info.filename);
    if qpath.exists() && !path.exists() {
        return CacheEntry {
            kind: "stt",
            id: info.name.to_string(),
            path: qpath.display().to_string(),
            manifest_version: ARTIFACT_MANIFEST_VERSION,
            expected_bytes: model_exact_or_approx(info),
            actual_bytes: fs::metadata(&qpath).ok().map(|m| m.len()),
            expected_sha256: None,
            state: VerifyState::Quarantined,
            last_error: Some("artifact is in quarantine".into()),
        };
    }

    let mut entry = CacheEntry {
        kind: "stt",
        id: info.name.to_string(),
        path: path.display().to_string(),
        manifest_version: ARTIFACT_MANIFEST_VERSION,
        expected_bytes: model_exact_or_approx(info),
        actual_bytes: None,
        expected_sha256: None,
        state: VerifyState::Missing,
        last_error: None,
    };

    entry.expected_sha256 = model::pinned_sha256(info.filename).map(|s| s.to_string());
    match model::ensure_model_verified_local(path, info) {
        Ok(()) => {
            entry.actual_bytes = fs::metadata(path).ok().map(|m| m.len());
            entry.state = if entry.expected_sha256.is_some() {
                VerifyState::Healthy
            } else {
                VerifyState::Unpinned
            };
        }
        Err(e) => {
            entry.last_error = Some(e.to_string());
            if path.exists() {
                entry.actual_bytes = fs::metadata(path).ok().map(|m| m.len());
                match quarantine_file(cache_dir, path, &e.to_string()) {
                    Ok(dest) => {
                        entry.path = dest.display().to_string();
                        entry.state = VerifyState::Quarantined;
                    }
                    Err(qe) => {
                        entry.state = VerifyState::DigestMismatch;
                        entry.last_error =
                            Some(format!("verify failed ({e}); quarantine failed ({qe})"));
                    }
                }
            } else {
                entry.state = VerifyState::Missing;
            }
        }
    }
    entry
}

fn model_exact_or_approx(info: &ModelInfo) -> Option<u64> {
    // Prefer reviewed exact size; fall back to approx only when no pin exists.
    model::pinned_exact_bytes(info.filename).or(Some(info.approx_bytes))
}

pub fn quarantine_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("quarantine")
}

/// Move an invalid artifact into quarantine with a reason file (no network).
pub fn quarantine_file(cache_dir: &Path, path: &Path, reason: &str) -> Result<PathBuf> {
    let qdir = quarantine_dir(cache_dir);
    fs::create_dir_all(&qdir).map_err(|e| EnvironmentError::DirectoryAccess {
        path: qdir.display().to_string(),
        reason: e.to_string(),
    })?;
    let name = path
        .file_name()
        .map(|s| s.to_os_string())
        .ok_or_else(|| UserError::Other {
            message: "cannot quarantine path without file name".into(),
        })?;
    let dest = qdir.join(&name);
    if path.exists() {
        fs::rename(path, &dest).map_err(|e| EnvironmentError::DirectoryAccess {
            path: dest.display().to_string(),
            reason: e.to_string(),
        })?;
    }
    let reason_path = dest.with_extension("quarantine-reason.txt");
    fs::write(&reason_path, format!("{reason}\n")).map_err(EnvironmentError::Io)?;
    Ok(dest)
}

/// Format human-readable status table.
pub fn format_status(entries: &[CacheEntry]) -> String {
    let mut out = String::from("Aurum cache inventory\n\n");
    out.push_str(&format!(
        "{:<8} {:<22} {:<12} {:>12}  {}\n",
        "KIND", "ID", "STATE", "BYTES", "PATH"
    ));
    for e in entries {
        out.push_str(&format!(
            "{:<8} {:<22} {:<12} {:>12}  {}\n",
            e.kind,
            e.id,
            format!("{:?}", e.state).to_ascii_lowercase(),
            e.actual_bytes
                .map(|b| b.to_string())
                .unwrap_or_else(|| "—".into()),
            e.path
        ));
    }
    out.push_str(
        "\nNote: `status` is cheap (size/existence). Use `aurum cache verify` for full digests.\n",
    );
    out
}

/// JSON for host health checks.
pub fn status_json(entries: &[CacheEntry]) -> Result<String> {
    serde_json::to_string_pretty(entries)
        .map_err(|e| crate::error::TranscriptionError::internal(format!("cache status json: {e}")))
}
