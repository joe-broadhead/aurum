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
        // JOE-1916 / F-003: use the shared secure output transaction (exclusive
        // temp, no-follow symlink policy, durable commit) instead of predictable
        // `*.json.tmp` + fs::write/rename.
        let json = self.to_json_pretty()?;
        crate::output::OutputTransaction::new(path, crate::output::CommitMode::NoClobber)
            .commit_bytes(json.as_bytes())
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

    // User notes are untrusted/user-supplied: path-tokenise and scrub token-shaped
    // material. They are NOT covered by the same privacy guarantee as machine fields.
    let mut user_notes = user_notes;
    if let Some(n) = user_notes.as_mut() {
        *n = crate::remote::redact_secret(&redact_path_str(n));
    }

    let mut redaction_notes = vec![
        "API keys redacted".into(),
        "source audio and transcripts never included".into(),
        "absolute home/config/cache/pack paths tokenised where known".into(),
        "no network requests performed while building this bundle".into(),
    ];
    if user_notes.is_some() {
        redaction_notes.push(
            "user_notes are untrusted free-form input: path/token redaction applied, but content is not privacy-guaranteed"
                .into(),
        );
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
        redaction_notes,
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
    // Longer roots first when both apply.
    let mut roots: Vec<(String, &str)> = Vec::new();
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            roots.push((home, "$HOME"));
        }
    }
    if let Ok(userprofile) = env::var("USERPROFILE") {
        if !userprofile.is_empty() {
            roots.push((userprofile, "$HOME"));
        }
    }
    if let Ok(xdg_config) = env::var("XDG_CONFIG_HOME") {
        if !xdg_config.is_empty() {
            roots.push((xdg_config, "$XDG_CONFIG_HOME"));
        }
    }
    if let Ok(xdg_cache) = env::var("XDG_CACHE_HOME") {
        if !xdg_cache.is_empty() {
            roots.push((xdg_cache, "$XDG_CACHE_HOME"));
        }
    }
    roots.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    for (root, token) in roots {
        if out.contains(&root) {
            out = out.replace(&root, token);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::SecretString;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn support_bundle_redacts_secrets_and_stays_offline() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.cache_dir = dir.path().join("cache");
        cfg.openrouter_api_key = Some(SecretString::new("sk-leaked-api-key-value"));
        let bundle = build_support_bundle(&cfg, Some("notes only".into()));
        let json = bundle.to_json_pretty().unwrap();
        assert!(!json.contains("sk-leaked-api-key-value"));
        assert!(json.contains("***") || bundle.config.openrouter_api_key.as_deref() == Some("***"));
        assert!(bundle
            .redaction_notes
            .iter()
            .any(|n| n.contains("no network")));
        assert!(bundle.doctor.checks.iter().any(|c| c.id == "offline"));
    }

    #[test]
    fn write_to_path_uses_secure_commit_and_noclobber() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bundle.json");
        let cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        let bundle = build_support_bundle(&cfg, None);
        bundle.write_to_path(&path).unwrap();
        assert!(path.is_file());
        // NoClobber: second write to same path must fail.
        let err = bundle.write_to_path(&path).unwrap_err();
        let msg = err.to_string().to_ascii_lowercase();
        assert!(
            msg.contains("exists") || msg.contains("clobber") || msg.contains("already"),
            "unexpected err: {msg}"
        );
        // No predictable leftover .json.tmp
        assert!(!dir.path().join("bundle.json.tmp").exists());
    }

    #[test]
    fn user_notes_token_shaped_material_redacted() {
        let dir = tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        let notes = "repro with Bearer sk-or-v1-canarynotesecretvalue99";
        let bundle = build_support_bundle(&cfg, Some(notes.into()));
        let json = bundle.to_json_pretty().unwrap();
        assert!(!json.contains("canarynotesecretvalue99"));
        assert!(bundle
            .redaction_notes
            .iter()
            .any(|n| n.contains("user_notes")));
    }

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
