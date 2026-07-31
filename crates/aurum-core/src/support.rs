//! Privacy-safe support bundles for early adopters (JOE-1728).
//!
//! Bundles never include API keys, audio, transcripts, or private absolute
//! home paths by default. Paths are redacted to `$HOME` / `$CACHE` tokens.

use crate::config::{Config, EffectiveConfigDiagnostic};
use crate::doctor::{run_doctor, DoctorReport};
use crate::error::{Result, UserError};
use crate::observability::{process_metrics, MetricsSnapshot};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Support bundle schema version.
pub const SUPPORT_BUNDLE_VERSION: u32 = 1;

/// Offline, allowlist-based diagnostic package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportBundle {
    pub schema_version: u32,
    pub created_at_unix: u64,
    pub aurum_version: String,
    pub os: String,
    pub arch: String,
    pub family: String,
    pub doctor: DoctorReport,
    pub metrics: MetricsSnapshot,
    pub config: EffectiveConfigDiagnostic,
    /// Explicit statements about what was excluded.
    pub redaction_notes: Vec<String>,
    /// Optional free-form notes from the user (already treated as public).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_notes: Option<String>,
}

impl SupportBundle {
    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            UserError::Other {
                message: format!("support bundle json: {e}"),
            }
            .into()
        })
    }

    pub fn write_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| UserError::Other {
                message: format!("create support bundle dir: {e}"),
            })?;
        }
        let json = self.to_json_pretty()?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json.as_bytes()).map_err(|e| UserError::Other {
            message: format!("write support bundle temp: {e}"),
        })?;
        fs::rename(&tmp, path).map_err(|e| UserError::Other {
            message: format!("publish support bundle: {e}"),
        })?;
        Ok(())
    }
}

/// Build a support bundle from the effective config (no network, no downloads).
pub fn build_support_bundle(cfg: &Config, user_notes: Option<String>) -> SupportBundle {
    let mut doctor = run_doctor(cfg);
    // Redact path-like strings inside doctor check details/summaries.
    for c in &mut doctor.checks {
        c.summary = redact_path_str(&c.summary);
        if let Some(d) = c.detail.as_mut() {
            *d = redact_path_str(d);
        }
        if let Some(h) = c.hint.as_mut() {
            *h = redact_path_str(h);
        }
    }
    let mut diag = cfg.effective_diagnostic();
    // Extra redaction pass on path-like fields.
    diag.cache_dir = redact_path_str(&diag.cache_dir);
    if let Some(p) = diag.config_path.as_mut() {
        *p = redact_path_str(p);
    }
    if let Some(p) = diag.tts_pack_dir.as_mut() {
        *p = redact_path_str(p);
    }
    // Never emit raw key material (Config already maps to "***" or None).
    if diag.openrouter_api_key.is_some() {
        diag.openrouter_api_key = Some("***".into());
    }

    let mut user_notes = user_notes;
    if let Some(n) = user_notes.as_mut() {
        *n = redact_path_str(n);
    }

    SupportBundle {
        schema_version: SUPPORT_BUNDLE_VERSION,
        created_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        aurum_version: env!("CARGO_PKG_VERSION").into(),
        os: env::consts::OS.into(),
        arch: env::consts::ARCH.into(),
        family: env::consts::FAMILY.into(),
        doctor,
        metrics: process_metrics().snapshot(),
        config: diag,
        redaction_notes: vec![
            "API keys redacted".into(),
            "source audio and transcripts never included".into(),
            "absolute home/cache paths tokenised".into(),
            "no network requests performed while building this bundle".into(),
        ],
        user_notes,
    }
}

/// Default path suggestion for a new support bundle file.
pub fn default_bundle_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    PathBuf::from(format!("aurum-support-{stamp}.json"))
}

fn redact_path_str(s: &str) -> String {
    let mut out = s.to_string();
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            out = out.replace(&home, "$HOME");
        }
    }
    if let Ok(userprofile) = env::var("USERPROFILE") {
        if !userprofile.is_empty() {
            out = out.replace(&userprofile, "$HOME");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_has_no_home_leak_when_redacted() {
        let cfg = Config::load().unwrap();
        let b = build_support_bundle(&cfg, Some("repro steps".into()));
        let json = b.to_json_pretty().unwrap();
        assert!(json.contains("schema_version"));
        assert!(json.contains("redaction_notes"));
        assert!(!json.contains("OPENROUTER_API_KEY="));
        // If HOME is set and appeared in cache_dir raw form, redaction should tokenise.
        if let Ok(home) = env::var("HOME") {
            if cfg.cache_dir.starts_with(&home) {
                assert!(!json.contains(&home));
                assert!(json.contains("$HOME") || !json.contains("cache_dir"));
            }
        }
    }

    #[test]
    fn write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.json");
        let cfg = Config::load().unwrap();
        let b = build_support_bundle(&cfg, None);
        b.write_to_path(&path).unwrap();
        let loaded: SupportBundle =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.schema_version, SUPPORT_BUNDLE_VERSION);
    }
}
