//! Named-hardware end-to-end performance programme (JOE-2218).
//!
//! Versioned reports, scenario catalogue, percentile helpers, and fail-closed
//! regression budgets. Download/network time is never mixed into local
//! inference budgets. Reports retain scenario IDs and timings only — no
//! transcripts, audio, or secrets.

use crate::error::{Result, UserError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Performance report schema version.
pub const PERF_SCHEMA_VERSION: u32 = 2;

/// Evidence / programme version.
pub const PERF_EVIDENCE_VERSION: &str = "0.0.22-perf-v1";

// ---------------------------------------------------------------------------
// Hardware identity (coarse product specs only)
// ---------------------------------------------------------------------------

/// Tier A family for release-gated local performance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HardwareTier {
    /// macOS arm64 (Apple Silicon).
    MacosArm64,
    /// Linux x86_64 GNU.
    LinuxX86_64Gnu,
    /// Windows x86_64 MSVC.
    WindowsX86_64Msvc,
}

impl HardwareTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MacosArm64 => "macos_arm64",
            Self::LinuxX86_64Gnu => "linux_x86_64_gnu",
            Self::WindowsX86_64Msvc => "windows_x86_64_msvc",
        }
    }

    pub fn all() -> &'static [HardwareTier] {
        &[
            Self::MacosArm64,
            Self::LinuxX86_64Gnu,
            Self::WindowsX86_64Msvc,
        ]
    }
}

/// Coarse named-hardware profile (no serials / usernames).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NamedHardwareProfile {
    pub profile_id: String,
    pub tier: HardwareTier,
    /// e.g. "Apple M2 Pro", "AMD EPYC …"
    pub cpu_label: String,
    pub core_count: u32,
    pub memory_gib: u32,
    /// e.g. "macOS 15.x", "Ubuntu 24.04 kernel 6.x", "Windows 11 build …"
    pub os_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_mode: Option<String>,
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

/// Stable scenario catalogue entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerfScenario {
    pub id: String,
    /// stt_local | tts_local | workflow | remote_info | governor
    pub kind: String,
    pub description: String,
    /// Whether this scenario is release-gated on the same named machine.
    pub release_gated: bool,
    /// Minimum measured repetitions (warmups excluded).
    pub min_repetitions: u32,
    /// Warmup runs excluded from samples.
    #[serde(default)]
    pub warmups: u32,
}

/// Built-in scenario catalogue required by JOE-2218.
pub fn perf_scenario_catalogue() -> Vec<PerfScenario> {
    let mut v = Vec::new();
    let stt_models = [
        "tiny-q5_1",
        "base",
        "large-v3-turbo",
        "profile_speed",
        "profile_balance",
        "profile_quality",
    ];
    let durations = [("30s", 5), ("5m", 5), ("long_form", 5)];
    for m in stt_models {
        for (d, reps) in durations {
            v.push(PerfScenario {
                id: format!("stt_local/{m}/{d}/warm"),
                kind: "stt_local".into(),
                description: format!("Local STT {m} {d} warm"),
                release_gated: m == "tiny-q5_1" || m == "base",
                min_repetitions: reps,
                warmups: 1,
            });
        }
        v.push(PerfScenario {
            id: format!("stt_local/{m}/cold_load"),
            kind: "stt_local".into(),
            description: format!("Cold process model load {m}"),
            release_gated: m == "tiny-q5_1",
            min_repetitions: 5,
            warmups: 0,
        });
        for conc in [1u32, 2, 4] {
            v.push(PerfScenario {
                id: format!("stt_local/{m}/concurrency_{conc}"),
                kind: "governor".into(),
                description: format!("STT concurrency {conc} with governor ({m})"),
                release_gated: m == "tiny-q5_1" && conc <= 2,
                min_repetitions: 5,
                warmups: 1,
            });
        }
    }
    for (model, voice) in [
        ("kitten-nano-int8", "Luna"),
        ("kokoro-82m-int8", "default"),
    ] {
        for phrase in ["short", "paragraph", "multi_chunk"] {
            v.push(PerfScenario {
                id: format!("tts_local/{model}/{voice}/{phrase}"),
                kind: "tts_local".into(),
                description: format!("Local TTS {model}/{voice} {phrase}"),
                release_gated: model == "kitten-nano-int8" && phrase == "short",
                min_repetitions: 5,
                warmups: 1,
            });
        }
    }
    for (id, desc, gated, reps) in [
        (
            "workflow/cli_stt_one_file",
            "One-file CLI STT decode+commit",
            true,
            20u32,
        ),
        (
            "workflow/batch_20_small",
            "Resumable batch of ≥20 small files",
            true,
            5,
        ),
        (
            "workflow/doctor_startup",
            "doctor / cache status startup",
            true,
            20,
        ),
        (
            "workflow/c_abi_job_overhead",
            "C ABI job start/poll/take overhead",
            false,
            20,
        ),
        (
            "workflow/long_form_mock_remote",
            "Long-form chunk orchestration with mock provider",
            false,
            5,
        ),
        (
            "remote_info/stt_upload_latency",
            "Remote STT informational latency",
            false,
            5,
        ),
    ] {
        v.push(PerfScenario {
            id: id.into(),
            kind: if id.starts_with("remote") {
                "remote_info".into()
            } else {
                "workflow".into()
            },
            description: desc.into(),
            release_gated: gated,
            min_repetitions: reps,
            warmups: if reps >= 20 { 2 } else { 1 },
        });
    }
    v
}

// ---------------------------------------------------------------------------
// Samples & report
// ---------------------------------------------------------------------------

/// One measured scenario result (warmups already excluded from samples).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerfScenarioResult {
    pub scenario_id: String,
    /// Wall times in milliseconds (measured samples only).
    pub samples_ms: Vec<f64>,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub mean_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_or_synth_duration_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtf_p50: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_rss_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steady_rss_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_wait_p50_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference_p50_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_ops_per_s: Option<f64>,
    #[serde(default)]
    pub concurrency: u32,
    #[serde(default)]
    pub warm: bool,
    /// Separated download/network ms (must not enter local inference budgets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_ms: Option<f64>,
    #[serde(default)]
    pub release_gated: bool,
}

impl PerfScenarioResult {
    pub fn from_samples(
        scenario_id: &str,
        mut samples_ms: Vec<f64>,
        audio_or_synth_duration_ms: Option<f64>,
        concurrency: u32,
        warm: bool,
        release_gated: bool,
    ) -> Result<Self> {
        if samples_ms.is_empty() {
            return Err(UserError::Other {
                message: format!("scenario '{scenario_id}' has no samples"),
            }
            .into());
        }
        for s in &samples_ms {
            if !s.is_finite() || *s < 0.0 {
                return Err(UserError::Other {
                    message: format!("scenario '{scenario_id}' has invalid sample {s}"),
                }
                .into());
            }
        }
        samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = percentile_sorted(&samples_ms, 0.50);
        let p95 = percentile_sorted(&samples_ms, 0.95);
        let mean = samples_ms.iter().sum::<f64>() / samples_ms.len() as f64;
        let rtf_p50 = audio_or_synth_duration_ms.map(|d| {
            if d <= 0.0 {
                0.0
            } else {
                p50 / d
            }
        });
        Ok(Self {
            scenario_id: scenario_id.into(),
            samples_ms,
            p50_ms: p50,
            p95_ms: p95,
            mean_ms: mean,
            audio_or_synth_duration_ms,
            rtf_p50,
            peak_rss_bytes: None,
            steady_rss_bytes: None,
            queue_wait_p50_ms: None,
            inference_p50_ms: None,
            throughput_ops_per_s: None,
            concurrency,
            warm,
            download_ms: None,
            release_gated,
        })
    }
}

/// Percentile on a **sorted** sample slice. `p` in [0, 1].
pub fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let p = p.clamp(0.0, 1.0);
    let rank = p * (sorted.len() as f64 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let w = rank - lo as f64;
        sorted[lo] * (1.0 - w) + sorted[hi] * w
    }
}

/// Full performance report for one named machine run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerfReport {
    pub schema_version: u32,
    pub evidence_version: String,
    pub hardware: NamedHardwareProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aurum_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rustc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_triple: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whisper_cpp_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onnx_runtime_version: Option<String>,
    /// model_id → sha256
    #[serde(default)]
    pub model_digests: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_state: Option<String>,
    pub scenarios: Vec<PerfScenarioResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl PerfReport {
    pub fn new(hardware: NamedHardwareProfile) -> Self {
        Self {
            schema_version: PERF_SCHEMA_VERSION,
            evidence_version: PERF_EVIDENCE_VERSION.into(),
            hardware,
            aurum_version: Some(env!("CARGO_PKG_VERSION").into()),
            commit: std::env::var("GITHUB_SHA")
                .or_else(|_| std::env::var("AURUM_BENCH_COMMIT"))
                .ok(),
            rustc: None,
            target_triple: Some(format!(
                "{}-{}",
                std::env::consts::ARCH,
                std::env::consts::OS
            )),
            build_profile: None,
            features: None,
            whisper_cpp_version: None,
            onnx_runtime_version: None,
            model_digests: BTreeMap::new(),
            cache_state: None,
            scenarios: Vec::new(),
            notes: None,
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read perf report {}: {e}", path.display()),
        })?;
        serde_json::from_str(&data).map_err(|e| {
            UserError::Other {
                message: format!("parse perf report: {e}"),
            }
            .into()
        })
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            UserError::Other {
                message: format!("serialize perf report: {e}"),
            }
            .into()
        })
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Performance report\n\n");
        out.push_str(&format!(
            "- **Evidence:** {}\n- **Profile:** `{}` ({})\n- **CPU:** {} ({} cores)\n- **Memory:** {} GiB\n- **OS:** {}\n",
            self.evidence_version,
            self.hardware.profile_id,
            self.hardware.tier.as_str(),
            self.hardware.cpu_label,
            self.hardware.core_count,
            self.hardware.memory_gib,
            self.hardware.os_label
        ));
        if let Some(ref c) = self.commit {
            out.push_str(&format!("- **Commit:** `{c}`\n"));
        }
        out.push_str(
            "\n| Scenario | p50 ms | p95 ms | RTF p50 | RSS | conc | gated |\n|----------|--------|--------|---------|-----|------|-------|\n",
        );
        let mut scenarios = self.scenarios.clone();
        scenarios.sort_by(|a, b| a.scenario_id.cmp(&b.scenario_id));
        for s in &scenarios {
            out.push_str(&format!(
                "| {} | {:.1} | {:.1} | {} | {} | {} | {} |\n",
                s.scenario_id,
                s.p50_ms,
                s.p95_ms,
                s.rtf_p50
                    .map(|r| format!("{r:.3}"))
                    .unwrap_or_else(|| "—".into()),
                s.peak_rss_bytes
                    .map(|b| format!("{:.0} MiB", b as f64 / (1024.0 * 1024.0)))
                    .unwrap_or_else(|| "—".into()),
                s.concurrency,
                s.release_gated
            ));
        }
        out.push('\n');
        out
    }
}

// ---------------------------------------------------------------------------
// Budgets
// ---------------------------------------------------------------------------

/// Per-scenario baseline budget on one named machine + model digest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerfScenarioBudget {
    pub scenario_id: String,
    pub baseline_p50_ms: f64,
    pub baseline_p95_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_rtf_p50: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_peak_rss_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_throughput_ops_per_s: Option<f64>,
    /// p50 wall/RTF regression fraction → warning (default 0.10).
    #[serde(default = "default_p50_warn")]
    pub max_p50_relative_warn: f64,
    /// p95 regression fraction → fail (default 0.15).
    #[serde(default = "default_p95_fail")]
    pub max_p95_relative_fail: f64,
    /// Peak RSS relative or absolute 256 MiB, whichever larger.
    #[serde(default = "default_rss_rel")]
    pub max_rss_relative_fail: f64,
    #[serde(default = "default_rss_abs")]
    pub max_rss_absolute_bytes: u64,
    #[serde(default = "default_tp_rel")]
    pub max_throughput_relative_drop: f64,
}

fn default_p50_warn() -> f64 {
    0.10
}
fn default_p95_fail() -> f64 {
    0.15
}
fn default_rss_rel() -> f64 {
    0.15
}
fn default_rss_abs() -> u64 {
    256 * 1024 * 1024
}
fn default_tp_rel() -> f64 {
    0.15
}

/// Committed performance budget file for one hardware profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerfBudget {
    pub schema_version: u32,
    pub evidence_version: String,
    pub hardware_profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_digest_pin: Option<String>,
    pub scenarios: Vec<PerfScenarioBudget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl PerfBudget {
    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read perf budget {}: {e}", path.display()),
        })?;
        serde_json::from_str(&data).map_err(|e| {
            UserError::Other {
                message: format!("parse perf budget: {e}"),
            }
            .into()
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerfSeverity {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerfFinding {
    pub severity: PerfSeverity,
    pub scenario_id: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerfComparison {
    pub hardware_profile_id: String,
    pub passed: bool,
    pub findings: Vec<PerfFinding>,
}

/// Compare candidate report to budget (same machine). Cross-machine never used to hide regressions.
pub fn compare_perf_budget(report: &PerfReport, budget: &PerfBudget) -> PerfComparison {
    let mut findings = Vec::new();

    if report.hardware.profile_id != budget.hardware_profile_id {
        findings.push(PerfFinding {
            severity: PerfSeverity::Fail,
            scenario_id: "*".into(),
            code: "hardware_mismatch".into(),
            message: format!(
                "report profile '{}' != budget '{}'",
                report.hardware.profile_id, budget.hardware_profile_id
            ),
        });
    }

    if let Some(ref pin) = budget.model_digest_pin {
        let any = report.model_digests.values().any(|d| d == pin);
        if !report.model_digests.is_empty() && !any {
            findings.push(PerfFinding {
                severity: PerfSeverity::Fail,
                scenario_id: "*".into(),
                code: "model_digest_mismatch".into(),
                message: format!("budget model_digest_pin {pin} not present in report digests"),
            });
        }
    }

    let by_id: BTreeMap<_, _> = report
        .scenarios
        .iter()
        .map(|s| (s.scenario_id.as_str(), s))
        .collect();

    for b in &budget.scenarios {
        let Some(cand) = by_id.get(b.scenario_id.as_str()) else {
            findings.push(PerfFinding {
                severity: PerfSeverity::Fail,
                scenario_id: b.scenario_id.clone(),
                code: "missing_scenario".into(),
                message: "budget scenario missing from candidate report".into(),
            });
            continue;
        };

        // p50 warn
        let p50_lim = b.baseline_p50_ms * (1.0 + b.max_p50_relative_warn);
        if cand.p50_ms > p50_lim + f64::EPSILON {
            findings.push(PerfFinding {
                severity: PerfSeverity::Warn,
                scenario_id: b.scenario_id.clone(),
                code: "p50_regression".into(),
                message: format!(
                    "p50 {:.1}ms exceeds warn {:.1}ms (baseline {:.1}, +{:.0}%)",
                    cand.p50_ms,
                    p50_lim,
                    b.baseline_p50_ms,
                    b.max_p50_relative_warn * 100.0
                ),
            });
        }

        // p95 fail
        let p95_lim = b.baseline_p95_ms * (1.0 + b.max_p95_relative_fail);
        if cand.p95_ms > p95_lim + f64::EPSILON {
            findings.push(PerfFinding {
                severity: PerfSeverity::Fail,
                scenario_id: b.scenario_id.clone(),
                code: "p95_regression".into(),
                message: format!(
                    "p95 {:.1}ms exceeds fail {:.1}ms (baseline {:.1}, +{:.0}%)",
                    cand.p95_ms,
                    p95_lim,
                    b.baseline_p95_ms,
                    b.max_p95_relative_fail * 100.0
                ),
            });
        }

        // RTF p50 warn (same relative as p50)
        if let (Some(base_rtf), Some(cand_rtf)) = (b.baseline_rtf_p50, cand.rtf_p50) {
            let lim = base_rtf * (1.0 + b.max_p50_relative_warn);
            if cand_rtf > lim + f64::EPSILON {
                findings.push(PerfFinding {
                    severity: PerfSeverity::Warn,
                    scenario_id: b.scenario_id.clone(),
                    code: "rtf_p50_regression".into(),
                    message: format!(
                        "RTF p50 {cand_rtf:.4} exceeds warn {lim:.4} (baseline {base_rtf:.4})"
                    ),
                });
            }
        }

        // Peak RSS fail
        if let (Some(base_rss), Some(cand_rss)) = (b.baseline_peak_rss_bytes, cand.peak_rss_bytes) {
            let rel_lim = (base_rss as f64 * (1.0 + b.max_rss_relative_fail)) as u64;
            let abs_lim = base_rss.saturating_add(b.max_rss_absolute_bytes);
            let lim = rel_lim.max(abs_lim);
            if cand_rss > lim {
                findings.push(PerfFinding {
                    severity: PerfSeverity::Fail,
                    scenario_id: b.scenario_id.clone(),
                    code: "rss_regression".into(),
                    message: format!(
                        "peak RSS {cand_rss} exceeds limit {lim} (baseline {base_rss})"
                    ),
                });
            }
        }

        // Throughput drop fail
        if let (Some(base_tp), Some(cand_tp)) =
            (b.baseline_throughput_ops_per_s, cand.throughput_ops_per_s)
        {
            let floor = base_tp * (1.0 - b.max_throughput_relative_drop);
            if cand_tp + f64::EPSILON < floor {
                findings.push(PerfFinding {
                    severity: PerfSeverity::Fail,
                    scenario_id: b.scenario_id.clone(),
                    code: "throughput_regression".into(),
                    message: format!(
                        "throughput {cand_tp:.3} ops/s below floor {floor:.3} (baseline {base_tp:.3})"
                    ),
                });
            }
        }
    }

    if findings.is_empty() {
        findings.push(PerfFinding {
            severity: PerfSeverity::Pass,
            scenario_id: "*".into(),
            code: "ok".into(),
            message: "all performance budget checks passed".into(),
        });
    }

    let passed = findings.iter().all(|f| f.severity != PerfSeverity::Fail);
    PerfComparison {
        hardware_profile_id: budget.hardware_profile_id.clone(),
        passed,
        findings,
    }
}

pub fn perf_budget_exit_code(cmp: &PerfComparison) -> i32 {
    if cmp.passed {
        0
    } else {
        1
    }
}

/// Documented Tier A profile placeholders (exact machines filled by operators).
pub fn tier_a_profile_templates() -> Vec<NamedHardwareProfile> {
    vec![
        NamedHardwareProfile {
            profile_id: "tier_a_macos_arm64".into(),
            tier: HardwareTier::MacosArm64,
            cpu_label: "Apple Silicon (exact chip recorded per run)".into(),
            core_count: 0,
            memory_gib: 0,
            os_label: "macOS (version recorded per run)".into(),
            power_mode: Some("performance_or_default".into()),
        },
        NamedHardwareProfile {
            profile_id: "tier_a_linux_x86_64_gnu".into(),
            tier: HardwareTier::LinuxX86_64Gnu,
            cpu_label: "x86_64 (exact CPU recorded per run)".into(),
            core_count: 0,
            memory_gib: 0,
            os_label: "Linux GNU (distro/kernel recorded per run)".into(),
            power_mode: None,
        },
        NamedHardwareProfile {
            profile_id: "tier_a_windows_x86_64_msvc".into(),
            tier: HardwareTier::WindowsX86_64Msvc,
            cpu_label: "x86_64 (exact CPU recorded per run)".into(),
            core_count: 0,
            memory_gib: 0,
            os_label: "Windows MSVC (build recorded per run)".into(),
            power_mode: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hardware() -> NamedHardwareProfile {
        NamedHardwareProfile {
            profile_id: "tier_a_macos_arm64".into(),
            tier: HardwareTier::MacosArm64,
            cpu_label: "Apple M2".into(),
            core_count: 8,
            memory_gib: 16,
            os_label: "macOS 15.0".into(),
            power_mode: None,
        }
    }

    #[test]
    fn percentiles() {
        let s = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((percentile_sorted(&s, 0.50) - 3.0).abs() < 1e-9);
        assert!(percentile_sorted(&s, 0.95) > 4.0);
        assert_eq!(percentile_sorted(&[42.0], 0.95), 42.0);
    }

    #[test]
    fn catalogue_has_tier_a_scenarios() {
        let c = perf_scenario_catalogue();
        assert!(c.len() > 20);
        assert!(c.iter().any(|s| s.id.contains("tiny-q5_1")));
        assert!(c.iter().any(|s| s.id.contains("tts_local")));
        assert!(c.iter().any(|s| s.release_gated));
        assert!(c.iter().any(|s| !s.release_gated));
    }

    #[test]
    fn budget_pass_and_p95_fail() {
        let mut report = PerfReport::new(sample_hardware());
        let ok = PerfScenarioResult::from_samples(
            "workflow/cli_stt_one_file",
            vec![100.0, 102.0, 101.0, 99.0, 100.0],
            Some(5000.0),
            1,
            true,
            true,
        )
        .unwrap();
        report.scenarios.push(ok);

        let budget = PerfBudget {
            schema_version: PERF_SCHEMA_VERSION,
            evidence_version: PERF_EVIDENCE_VERSION.into(),
            hardware_profile_id: "tier_a_macos_arm64".into(),
            model_digest_pin: None,
            scenarios: vec![PerfScenarioBudget {
                scenario_id: "workflow/cli_stt_one_file".into(),
                baseline_p50_ms: 100.0,
                baseline_p95_ms: 110.0,
                baseline_rtf_p50: Some(0.02),
                baseline_peak_rss_bytes: None,
                baseline_throughput_ops_per_s: None,
                max_p50_relative_warn: 0.10,
                max_p95_relative_fail: 0.15,
                max_rss_relative_fail: 0.15,
                max_rss_absolute_bytes: 256 * 1024 * 1024,
                max_throughput_relative_drop: 0.15,
            }],
            notes: None,
        };
        let cmp = compare_perf_budget(&report, &budget);
        assert!(cmp.passed, "{:?}", cmp.findings);

        // Inject regression
        report.scenarios[0] = PerfScenarioResult::from_samples(
            "workflow/cli_stt_one_file",
            vec![200.0, 210.0, 220.0, 230.0, 250.0],
            Some(5000.0),
            1,
            true,
            true,
        )
        .unwrap();
        let cmp2 = compare_perf_budget(&report, &budget);
        assert!(!cmp2.passed);
        assert_eq!(perf_budget_exit_code(&cmp2), 1);
        assert!(cmp2.findings.iter().any(|f| f.code == "p95_regression"));
    }

    #[test]
    fn hardware_mismatch_fails() {
        let report = PerfReport::new(sample_hardware());
        let budget = PerfBudget {
            schema_version: PERF_SCHEMA_VERSION,
            evidence_version: PERF_EVIDENCE_VERSION.into(),
            hardware_profile_id: "tier_a_linux_x86_64_gnu".into(),
            model_digest_pin: None,
            scenarios: vec![],
            notes: None,
        };
        let cmp = compare_perf_budget(&report, &budget);
        assert!(!cmp.passed);
        assert!(cmp.findings.iter().any(|f| f.code == "hardware_mismatch"));
    }

    #[test]
    fn markdown_deterministic_order() {
        let mut report = PerfReport::new(sample_hardware());
        report.scenarios.push(
            PerfScenarioResult::from_samples("b", vec![2.0], None, 1, true, false).unwrap(),
        );
        report.scenarios.push(
            PerfScenarioResult::from_samples("a", vec![1.0], None, 1, true, false).unwrap(),
        );
        let md = report.to_markdown();
        let ia = md.find("| a |").unwrap();
        let ib = md.find("| b |").unwrap();
        assert!(ia < ib);
    }
}
