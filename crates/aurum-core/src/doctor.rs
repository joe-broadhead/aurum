//! Read-only system / config / cache / capability diagnostics (JOE-1628).
//!
//! Never performs downloads, network calls, or secret printing.

use crate::config::Config;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Schema version for doctor JSON.
pub const DOCTOR_SCHEMA_VERSION: u32 = 1;

/// One diagnostic check result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorCheck {
    pub id: String,
    pub ok: bool,
    pub severity: DoctorSeverity,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    Info,
    Warn,
    Error,
}

/// Full doctor report (redacted; safe to print).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub aurum_version: String,
    pub target: String,
    pub features: Vec<String>,
    pub checks: Vec<DoctorCheck>,
    pub ok: bool,
}

impl DoctorReport {
    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| crate::error::TranscriptionError::internal(format!("doctor json: {e}")))
    }

    pub fn format_human(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "aurum doctor  version={}  target={}\n",
            self.aurum_version, self.target
        ));
        out.push_str(&format!("features: {}\n\n", self.features.join(", ")));
        for c in &self.checks {
            let mark = if c.ok { "ok" } else { "!!" };
            out.push_str(&format!("[{mark}] {} — {}\n", c.id, c.summary));
            if let Some(d) = &c.detail {
                out.push_str(&format!("     {d}\n"));
            }
            if let Some(h) = &c.hint {
                out.push_str(&format!("     Hint: {h}\n"));
            }
        }
        out.push_str(&format!(
            "\noverall: {}\n",
            if self.ok { "healthy" } else { "issues found" }
        ));
        out
    }
}

/// Run the standard doctor suite using the effective config.
pub fn run_doctor(cfg: &Config) -> DoctorReport {
    let mut checks = Vec::new();
    #[cfg(feature = "tts")]
    let features = vec!["stt".into(), "cleanup".into(), "tts".into()];
    #[cfg(not(feature = "tts"))]
    let features = vec!["stt".into(), "cleanup".into()];

    checks.push(DoctorCheck {
        id: "version".into(),
        ok: true,
        severity: DoctorSeverity::Info,
        summary: format!("aurum-core {}", env!("CARGO_PKG_VERSION")),
        detail: Some(format!("target={}", std::env::consts::ARCH)),
        hint: None,
    });

    // Config validate.
    match cfg.validate() {
        Ok(()) => checks.push(DoctorCheck {
            id: "config".into(),
            ok: true,
            severity: DoctorSeverity::Info,
            summary: "configuration validates".into(),
            detail: Some(format!(
                "provider={} tts_model={} cache={}",
                cfg.provider,
                cfg.tts_model,
                cfg.cache_dir.display()
            )),
            hint: None,
        }),
        Err(e) => checks.push(DoctorCheck {
            id: "config".into(),
            ok: false,
            severity: DoctorSeverity::Error,
            summary: "configuration invalid".into(),
            detail: Some(e.to_string()),
            hint: Some("fix config.toml or environment overrides".into()),
        }),
    }

    // Cache directory.
    checks.push(dir_check("cache_dir", &cfg.cache_dir, true));

    // FFmpeg (STT path).
    match which::which("ffmpeg") {
        Ok(p) => checks.push(DoctorCheck {
            id: "ffmpeg".into(),
            ok: true,
            severity: DoctorSeverity::Info,
            summary: "ffmpeg found on PATH".into(),
            detail: Some(p.display().to_string()),
            hint: None,
        }),
        Err(_) => checks.push(DoctorCheck {
            id: "ffmpeg".into(),
            ok: false,
            severity: DoctorSeverity::Warn,
            summary: "ffmpeg not found on PATH".into(),
            detail: Some("required for local STT file decode".into()),
            hint: Some(
                "install ffmpeg (brew/apt/winget) or use PCM/WAV direct paths where supported"
                    .into(),
            ),
        }),
    }

    // Capability surface (static).
    let stt = crate::capabilities::local_whisper_capabilities(
        cfg.model
            .as_deref()
            .unwrap_or(crate::config::DEFAULT_LOCAL_MODEL),
    );
    checks.push(DoctorCheck {
        id: "capabilities_stt".into(),
        ok: true,
        severity: DoctorSeverity::Info,
        summary: format!(
            "local STT capabilities declared (timestamps_reliable={})",
            stt.timestamps_reliable
        ),
        detail: Some(format!("formats={}", stt.output_formats.join(","))),
        hint: None,
    });

    #[cfg(feature = "tts")]
    {
        let tts = crate::capabilities::local_tts_capabilities(&cfg.tts_model);
        checks.push(DoctorCheck {
            id: "capabilities_tts".into(),
            ok: true,
            severity: DoctorSeverity::Info,
            summary: format!("local TTS capabilities for model {}", tts.model),
            detail: Some(format!(
                "network={} local_only_ok={}",
                tts.requires_network, tts.local_only_ok
            )),
            hint: None,
        });
    }

    // Secrets redaction probe.
    let diag = cfg.effective_diagnostic();
    let key_ok = diag
        .openrouter_api_key
        .as_deref()
        .map(|k| k == "***" || k.is_empty())
        .unwrap_or(true);
    checks.push(DoctorCheck {
        id: "secrets_redacted".into(),
        ok: key_ok,
        severity: if key_ok {
            DoctorSeverity::Info
        } else {
            DoctorSeverity::Error
        },
        summary: if key_ok {
            "config diagnostics redact secrets".into()
        } else {
            "config diagnostics leaked a secret".into()
        },
        detail: None,
        hint: None,
    });

    // Disk free space (best-effort).
    checks.push(disk_space_check(&cfg.cache_dir));

    // Writable cache probe (JOE-1783): create/delete a tiny file when possible.
    checks.push(cache_writable_check(&cfg.cache_dir));

    // Explicit offline contract: default doctor never downloads or opens network.
    checks.push(DoctorCheck {
        id: "offline".into(),
        ok: true,
        severity: DoctorSeverity::Info,
        summary: "doctor performed no network or download".into(),
        detail: Some(
            "default doctor is offline-only; use explicit remote commands for connectivity checks"
                .into(),
        ),
        hint: None,
    });

    let ok = checks
        .iter()
        .all(|c| c.ok || matches!(c.severity, DoctorSeverity::Info | DoctorSeverity::Warn));

    DoctorReport {
        schema_version: DOCTOR_SCHEMA_VERSION,
        aurum_version: env!("CARGO_PKG_VERSION").into(),
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        features,
        checks,
        ok,
    }
}

fn dir_check(id: &str, path: &Path, create_ok: bool) -> DoctorCheck {
    if path.as_os_str().is_empty() {
        return DoctorCheck {
            id: id.into(),
            ok: false,
            severity: DoctorSeverity::Error,
            summary: format!("{id} is empty"),
            detail: None,
            hint: Some("set a writable cache directory".into()),
        };
    }
    if path.exists() {
        let meta = std::fs::metadata(path);
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        if is_dir {
            DoctorCheck {
                id: id.into(),
                ok: true,
                severity: DoctorSeverity::Info,
                summary: format!("{id} exists"),
                detail: Some(path.display().to_string()),
                hint: None,
            }
        } else {
            DoctorCheck {
                id: id.into(),
                ok: false,
                severity: DoctorSeverity::Error,
                summary: format!("{id} is not a directory"),
                detail: Some(path.display().to_string()),
                hint: None,
            }
        }
    } else if create_ok {
        DoctorCheck {
            id: id.into(),
            ok: true,
            severity: DoctorSeverity::Warn,
            summary: format!("{id} does not exist yet (will be created on use)"),
            detail: Some(path.display().to_string()),
            hint: None,
        }
    } else {
        DoctorCheck {
            id: id.into(),
            ok: false,
            severity: DoctorSeverity::Error,
            summary: format!("{id} missing"),
            detail: Some(path.display().to_string()),
            hint: None,
        }
    }
}

fn disk_space_check(path: &Path) -> DoctorCheck {
    let probe = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(path);
    if !(probe.exists() || path.exists()) {
        return DoctorCheck {
            id: "disk".into(),
            ok: false,
            severity: DoctorSeverity::Warn,
            summary: "cache path parent not found".into(),
            detail: Some(probe.display().to_string()),
            hint: Some("create the directory or choose another cache root".into()),
        };
    }
    // Best-effort free-space probe (same helper downloads use for preflight).
    match crate::download::available_disk_bytes(if probe.exists() { probe } else { path }) {
        Some(free) => {
            const WARN_BELOW: u64 = 512 * 1024 * 1024; // 512 MiB
            let mb = free / (1024 * 1024);
            if free < WARN_BELOW {
                DoctorCheck {
                    id: "disk".into(),
                    ok: true,
                    severity: DoctorSeverity::Warn,
                    summary: format!("low free space (~{mb} MiB)"),
                    detail: Some(probe.display().to_string()),
                    hint: Some(
                        "model downloads need budget + 64 MiB headroom; free space or shrink cache"
                            .into(),
                    ),
                }
            } else {
                DoctorCheck {
                    id: "disk".into(),
                    ok: true,
                    severity: DoctorSeverity::Info,
                    summary: format!("free space ~{mb} MiB"),
                    detail: Some(probe.display().to_string()),
                    hint: None,
                }
            }
        }
        None => DoctorCheck {
            id: "disk".into(),
            ok: true,
            severity: DoctorSeverity::Info,
            summary: "cache path parent is reachable".into(),
            detail: Some(probe.display().to_string()),
            hint: None,
        },
    }
}

fn cache_writable_check(path: &Path) -> DoctorCheck {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    // JOE-1916 / F-003: exclusive create_new with unpredictable name; never follow
    // a pre-existing symlink at a predictable probe path. Always remove on exit.
    let create = || -> std::io::Result<()> {
        fs::create_dir_all(path)?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let probe = path.join(format!(
            ".aurum-doctor-write-{}-{nanos}",
            std::process::id()
        ));
        // Guard: if a symlink already exists at our random name (astronomically rare),
        // refuse rather than following it.
        if probe.symlink_metadata().is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "probe path unexpectedly exists",
            ));
        }
        let write_result = (|| -> std::io::Result<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)?;
            f.write_all(b"ok")?;
            f.sync_all()?;
            Ok(())
        })();
        let _ = fs::remove_file(&probe);
        write_result
    };
    match create() {
        Ok(()) => DoctorCheck {
            id: "cache_writable".into(),
            ok: true,
            severity: DoctorSeverity::Info,
            summary: "cache directory is writable".into(),
            detail: Some(path.display().to_string()),
            hint: None,
        },
        Err(e) => DoctorCheck {
            id: "cache_writable".into(),
            ok: false,
            severity: DoctorSeverity::Error,
            summary: "cache directory is not writable".into(),
            detail: Some(format!("{}: {e}", path.display())),
            hint: Some("fix permissions or choose another cache root".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::secret::SecretString;
    use tempfile::tempdir;

    #[test]
    fn doctor_is_offline_and_includes_core_checks() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.cache_dir = dir.path().join("cache");
        let report = run_doctor(&cfg);
        let ids: Vec<_> = report.checks.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"version"));
        assert!(ids.contains(&"config"));
        assert!(ids.contains(&"cache_dir"));
        assert!(ids.contains(&"ffmpeg"));
        assert!(ids.contains(&"secrets_redacted"));
        assert!(ids.contains(&"offline"));
        assert!(ids.contains(&"cache_writable"));
        assert!(report
            .checks
            .iter()
            .any(|c| c.id == "offline" && c.ok && c.summary.contains("no network")));
        // JSON must not contain secret-shaped key material from a set key.
        cfg.openrouter_api_key = Some(SecretString::new("sk-super-secret-token-value"));
        let report2 = run_doctor(&cfg);
        let json = report2.to_json_pretty().unwrap();
        assert!(!json.contains("sk-super-secret-token-value"));
        assert!(report2
            .checks
            .iter()
            .any(|c| c.id == "secrets_redacted" && c.ok));
    }

    #[test]
    fn doctor_flags_unwritable_cache() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        // Point at a non-directory path so create_dir_all fails when a file exists.
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        cfg.cache_dir = file;
        let report = run_doctor(&cfg);
        let w = report
            .checks
            .iter()
            .find(|c| c.id == "cache_writable")
            .expect("cache_writable check");
        assert!(!w.ok);
    }

    #[test]
    fn cache_writable_probe_does_not_clobber_preexisting_symlink_target() {
        // Ensure a fixed-name attacker path is not used; probe is random exclusive.
        let dir = tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let victim = dir.path().join("victim-secret");
        std::fs::write(&victim, b"keep-me").unwrap();
        // Plant a predictable-name symlink like the old probe path.
        let decoy = cache.join(".aurum-doctor-write-probe");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&victim, &decoy).unwrap();
        }
        #[cfg(not(unix))]
        {
            // On Windows, skip symlink plant if privileges missing; still assert writable.
            let _ = decoy;
        }
        let check = cache_writable_check(&cache);
        assert!(check.ok, "writable cache should pass: {:?}", check);
        // Victim must be intact (we never open the decoy path).
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep-me");
        // No leftover random probe files (ignore the planted fixed-name decoy).
        let leftovers: Vec<_> = std::fs::read_dir(&cache)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(".aurum-doctor-write-") && name != ".aurum-doctor-write-probe"
            })
            .collect();
        assert!(leftovers.is_empty(), "probe leftovers: {leftovers:?}");
    }

    #[test]
    fn doctor_runs_on_defaults() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load().unwrap();
        cfg.cache_dir = dir.path().to_path_buf();
        let r = run_doctor(&cfg);
        assert_eq!(r.schema_version, DOCTOR_SCHEMA_VERSION);
        assert!(!r.checks.is_empty());
        assert!(r.to_json_pretty().unwrap().contains("aurum_version"));
        assert!(r.format_human().contains("aurum doctor"));
    }
}
