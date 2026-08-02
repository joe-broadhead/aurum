//! Versioned STT quality observatory (JOE-2216).
//!
//! Production-grade corpus schema, machine-readable reports, Markdown scorecards,
//! and fail-closed baseline budget comparison. CI uses the redistributable core;
//! larger licensed speech is fetched by a documented recipe and never required
//! as private Plaud material.

use crate::error::{Result, UserError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Observatory report / corpus schema version for the production programme.
pub const OBSERVATORY_SCHEMA_VERSION: u32 = 1;

/// Evidence pack identifier written into profile recommendations after review.
pub const STT_OBSERVATORY_EVIDENCE_VERSION: &str = "0.0.22-observatory-v1";

/// Normalization policy identifier (must match scoring path).
pub const NORMALIZATION_POLICY_VERSION: &str = "normalize_v1_lower_alnum_ws";

// ---------------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------------

/// How a fixture asset is obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetResolution {
    /// Checked into the repository under a relative path.
    Redistributable,
    /// Obtained via the documented fetch/prepare script; not in CI by default.
    ExternalFetch,
}

/// One real-speech (or control) fixture in the observatory corpus.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservatoryFixture {
    pub id: String,
    /// Relative path under corpus root, or external asset key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// Optional expected SHA-256 of the audio bytes (when present locally).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_sha256: Option<String>,
    /// Approximate duration in seconds (for coverage accounting).
    #[serde(default)]
    pub duration_secs: f64,
    pub language: String,
    /// Reference transcript (empty for silence / non-speech controls).
    pub reference: String,
    /// Normalization policy id applied before WER/CER.
    #[serde(default = "default_norm_policy")]
    pub normalization_policy: String,
    /// Scenario tags: clean, lecture, noisy, accent_*, numbers, silence, long_form, multilingual, …
    #[serde(default)]
    pub tags: Vec<String>,
    /// Distinct speaker id within the corpus (opaque token).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<String>,
    /// Licensing / provenance note (required for production fixtures).
    pub license: String,
    /// Provenance source (dataset name, URL family, synthetic generator).
    #[serde(default)]
    pub provenance: String,
    pub asset_resolution: AssetResolution,
    /// Whether the fixture may be redistributed with the repo.
    #[serde(default)]
    pub redistributable: bool,
    /// Use restrictions for operators (e.g. "research only", "no commercial retrain").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_restrictions: Option<String>,
    #[serde(default = "default_true")]
    pub timestamps_expected_reliable: bool,
    /// Optional word-level timing reference path (relative).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_reference: Option<String>,
}

fn default_norm_policy() -> String {
    NORMALIZATION_POLICY_VERSION.into()
}

fn default_true() -> bool {
    true
}

/// Versioned observatory corpus manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservatoryCorpus {
    pub schema_version: u32,
    pub name: String,
    /// Human/product corpus version string (e.g. `observatory-core-v1`).
    pub corpus_version: String,
    #[serde(default)]
    pub description: String,
    pub fixtures: Vec<ObservatoryFixture>,
}

impl ObservatoryCorpus {
    /// Validate structural and coverage contracts for the production programme.
    ///
    /// Coverage minima apply when `enforce_production_coverage` is true (full
    /// production corpus). Redistributable core may pass with lower coverage
    /// when `enforce_production_coverage` is false.
    pub fn validate(&self, enforce_production_coverage: bool) -> Result<CorpusCoverage> {
        if self.schema_version != OBSERVATORY_SCHEMA_VERSION {
            return Err(UserError::Other {
                message: format!(
                    "unsupported observatory corpus schema_version {} (expected {OBSERVATORY_SCHEMA_VERSION})",
                    self.schema_version
                ),
            }
            .into());
        }
        if self.name.trim().is_empty() {
            return Err(UserError::Other {
                message: "observatory corpus name must be non-empty".into(),
            }
            .into());
        }
        if self.fixtures.is_empty() {
            return Err(UserError::Other {
                message: "observatory corpus has no fixtures".into(),
            }
            .into());
        }

        let mut ids = std::collections::BTreeSet::new();
        let mut speakers = std::collections::BTreeSet::new();
        let mut total_secs = 0.0f64;
        let mut tags_seen = std::collections::BTreeSet::new();
        let mut long_form = 0u32;
        let mut accents = std::collections::BTreeSet::new();

        for f in &self.fixtures {
            if f.id.trim().is_empty() {
                return Err(UserError::Other {
                    message: "fixture id must be non-empty".into(),
                }
                .into());
            }
            if !ids.insert(f.id.clone()) {
                return Err(UserError::Other {
                    message: format!("duplicate fixture id '{}'", f.id),
                }
                .into());
            }
            if f.license.trim().is_empty() {
                return Err(UserError::Other {
                    message: format!("fixture '{}' missing license/provenance", f.id),
                }
                .into());
            }
            if f.duration_secs < 0.0 || !f.duration_secs.is_finite() {
                return Err(UserError::Other {
                    message: format!("fixture '{}' has invalid duration_secs", f.id),
                }
                .into());
            }
            // Bound pathological manifests.
            if f.reference.len() > 2_000_000 {
                return Err(UserError::Other {
                    message: format!("fixture '{}' reference exceeds size bound", f.id),
                }
                .into());
            }
            total_secs += f.duration_secs;
            if let Some(ref sp) = f.speaker_id {
                if !sp.is_empty() {
                    speakers.insert(sp.clone());
                }
            }
            for t in &f.tags {
                tags_seen.insert(t.to_ascii_lowercase());
                if t.to_ascii_lowercase().starts_with("accent_") {
                    accents.insert(t.to_ascii_lowercase());
                }
            }
            if f.tags.iter().any(|t| {
                let l = t.to_ascii_lowercase();
                l == "long_form" || l == "long-form" || l == "longform"
            }) || f.duration_secs > 600.0
            {
                long_form += 1;
            }
        }

        let coverage = CorpusCoverage {
            fixture_count: self.fixtures.len(),
            total_duration_secs: total_secs,
            speaker_count: speakers.len(),
            accent_tag_count: accents.len(),
            long_form_count: long_form,
            has_silence: tags_seen.iter().any(|t| t == "silence"),
            has_noisy: tags_seen
                .iter()
                .any(|t| t == "noisy" || t == "noise" || t == "reverberant"),
            has_lecture: tags_seen
                .iter()
                .any(|t| t == "lecture" || t == "presentation"),
            has_conversational: tags_seen
                .iter()
                .any(|t| t == "conversational" || t == "clean" || t == "conversation"),
            has_numbers: tags_seen
                .iter()
                .any(|t| t == "numbers" || t == "dates" || t == "acronyms"),
            has_multilingual: tags_seen
                .iter()
                .any(|t| t == "multilingual" || t == "code_switch" || t == "code-switching"),
            has_low_volume: tags_seen
                .iter()
                .any(|t| t == "low_volume" || t == "low-volume" || t == "pause"),
            tags: tags_seen.into_iter().collect(),
        };

        if enforce_production_coverage {
            coverage.require_production_minima()?;
        }

        Ok(coverage)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read observatory corpus {}: {e}", path.display()),
        })?;
        // Bound parse size (~32 MiB JSON).
        if data.len() > 32 * 1024 * 1024 {
            return Err(UserError::Other {
                message: format!(
                    "observatory corpus {} exceeds 32 MiB size bound",
                    path.display()
                ),
            }
            .into());
        }
        let corpus: Self = serde_json::from_str(&data).map_err(|e| UserError::Other {
            message: format!("parse observatory corpus: {e}"),
        })?;
        Ok(corpus)
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            UserError::Other {
                message: format!("serialize observatory corpus: {e}"),
            }
            .into()
        })
    }
}

/// Aggregate coverage metrics for documentation and gates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CorpusCoverage {
    pub fixture_count: usize,
    pub total_duration_secs: f64,
    pub speaker_count: usize,
    pub accent_tag_count: usize,
    pub long_form_count: u32,
    pub has_silence: bool,
    pub has_noisy: bool,
    pub has_lecture: bool,
    pub has_conversational: bool,
    pub has_numbers: bool,
    pub has_multilingual: bool,
    pub has_low_volume: bool,
    pub tags: Vec<String>,
}

impl CorpusCoverage {
    /// Production minima from JOE-2216 corpus contract.
    pub fn require_production_minima(&self) -> Result<()> {
        let mut missing = Vec::new();
        if self.total_duration_secs < 60.0 * 60.0 {
            missing.push(format!(
                "duration {:.1}s < 3600s (60 minutes)",
                self.total_duration_secs
            ));
        }
        if self.speaker_count < 20 {
            missing.push(format!("speakers {} < 20", self.speaker_count));
        }
        if self.accent_tag_count < 4 {
            missing.push(format!("accent tags {} < 4", self.accent_tag_count));
        }
        if self.long_form_count < 3 {
            missing.push(format!("long-form fixtures {} < 3", self.long_form_count));
        }
        if !self.has_silence {
            missing.push("silence control".into());
        }
        if !self.has_noisy {
            missing.push("noisy/reverberant".into());
        }
        if !self.has_lecture {
            missing.push("lecture/presentation".into());
        }
        if !self.has_conversational {
            missing.push("conversational/clean".into());
        }
        if !self.has_numbers {
            missing.push("numbers/dates/acronyms".into());
        }
        if !self.has_multilingual {
            missing.push("multilingual/code-switching".into());
        }
        if !self.has_low_volume {
            missing.push("low_volume/pause".into());
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(UserError::Other {
                message: format!(
                    "production corpus coverage incomplete: {}",
                    missing.join("; ")
                ),
            }
            .into())
        }
    }
}

// ---------------------------------------------------------------------------
// Per-fixture metrics & report
// ---------------------------------------------------------------------------

/// Extended per-fixture STT metrics for the observatory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservatoryFixtureScore {
    pub fixture_id: String,
    pub wer: f64,
    pub cer: f64,
    pub empty_hypothesis: bool,
    pub silence_false_positive: bool,
    pub repetition_ratio: f64,
    /// hyp_words / max(ref_words, 1)
    pub length_ratio: f64,
    pub ref_words: usize,
    pub hyp_words: usize,
    /// Scenario tags copied from the fixture for grouping.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Mean absolute timestamp alignment error in seconds when reference timing exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_mae_secs: Option<f64>,
    /// Long-form boundary deletion/duplication score in [0, 1] (0 = clean).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_error: Option<f64>,
    /// Wall-clock processing seconds (for performance cross-link).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing_secs: Option<f64>,
    /// Real-time factor when audio duration is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtf: Option<f64>,
}

/// Machine identity for a retained report (no serial numbers / usernames).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RunIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aurum_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub timestamps: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_warm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corpus_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization_policy: Option<String>,
}

/// Versioned observatory JSON report (no raw hypotheses or private paths).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservatoryReport {
    pub schema_version: u32,
    pub evidence_version: String,
    pub corpus_name: String,
    pub corpus_version: String,
    pub model: String,
    pub backend_kind: String,
    #[serde(default)]
    pub identity: RunIdentity,
    pub scores: Vec<ObservatoryFixtureScore>,
    pub mean_wer: f64,
    pub mean_cer: f64,
    pub silence_false_positives: u32,
    pub mean_repetition_ratio: f64,
    pub mean_length_ratio: f64,
    /// Scenario → mean WER.
    #[serde(default)]
    pub scenario_mean_wer: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl ObservatoryReport {
    pub fn from_scores(
        corpus: &ObservatoryCorpus,
        model: &str,
        backend_kind: &str,
        mut scores: Vec<ObservatoryFixtureScore>,
        identity: RunIdentity,
    ) -> Self {
        // Deterministic ordering.
        scores.sort_by(|a, b| a.fixture_id.cmp(&b.fixture_id));
        let n = scores.len().max(1) as f64;
        let mean_wer = scores.iter().map(|s| s.wer).sum::<f64>() / n;
        let mean_cer = scores.iter().map(|s| s.cer).sum::<f64>() / n;
        let silence_false_positives =
            scores.iter().filter(|s| s.silence_false_positive).count() as u32;
        let rep: Vec<f64> = scores
            .iter()
            .filter(|s| s.hyp_words > 0)
            .map(|s| s.repetition_ratio)
            .collect();
        let mean_repetition_ratio = if rep.is_empty() {
            0.0
        } else {
            rep.iter().sum::<f64>() / rep.len() as f64
        };
        let mean_length_ratio = scores.iter().map(|s| s.length_ratio).sum::<f64>() / n;
        let scenario_mean_wer = scenario_group_means(&scores);

        let mut identity = identity;
        if identity.corpus_version.is_none() {
            identity.corpus_version = Some(corpus.corpus_version.clone());
        }
        if identity.normalization_policy.is_none() {
            identity.normalization_policy = Some(NORMALIZATION_POLICY_VERSION.into());
        }
        if identity.model_id.is_none() {
            identity.model_id = Some(model.into());
        }

        Self {
            schema_version: OBSERVATORY_SCHEMA_VERSION,
            evidence_version: STT_OBSERVATORY_EVIDENCE_VERSION.into(),
            corpus_name: corpus.name.clone(),
            corpus_version: corpus.corpus_version.clone(),
            model: model.into(),
            backend_kind: backend_kind.into(),
            identity,
            scores,
            mean_wer,
            mean_cer,
            silence_false_positives,
            mean_repetition_ratio,
            mean_length_ratio,
            scenario_mean_wer,
            notes: None,
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read observatory report {}: {e}", path.display()),
        })?;
        serde_json::from_str(&data).map_err(|e| {
            UserError::Other {
                message: format!("parse observatory report: {e}"),
            }
            .into()
        })
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            UserError::Other {
                message: format!("serialize observatory report: {e}"),
            }
            .into()
        })
    }

    /// Human-readable Markdown scorecard (deterministic section order).
    pub fn to_markdown_scorecard(&self) -> String {
        let mut out = String::new();
        out.push_str("# STT quality scorecard\n\n");
        out.push_str(&format!(
            "- **Evidence version:** {}\n- **Corpus:** {} ({})\n- **Model:** `{}`\n- **Backend:** {}\n",
            self.evidence_version, self.corpus_name, self.corpus_version, self.model, self.backend_kind
        ));
        if let Some(ref hw) = self.identity.hardware_profile {
            out.push_str(&format!("- **Hardware profile:** `{hw}`\n"));
        }
        if let Some(ref commit) = self.identity.commit {
            out.push_str(&format!("- **Commit:** `{commit}`\n"));
        }
        out.push_str(&format!(
            "\n## Aggregate\n\n| Metric | Value |\n|--------|-------|\n| mean WER | {:.4} |\n| mean CER | {:.4} |\n| silence FP | {} |\n| mean repetition | {:.4} |\n| mean length ratio | {:.4} |\n",
            self.mean_wer,
            self.mean_cer,
            self.silence_false_positives,
            self.mean_repetition_ratio,
            self.mean_length_ratio
        ));
        out.push_str(
            "\n## Scenario mean WER\n\n| Scenario | mean WER |\n|----------|----------|\n",
        );
        for (k, v) in &self.scenario_mean_wer {
            out.push_str(&format!("| {k} | {v:.4} |\n"));
        }
        out.push_str("\n## Per-fixture\n\n| Fixture | WER | CER | silence FP | rep |\n|---------|-----|-----|------------|-----|\n");
        for s in &self.scores {
            out.push_str(&format!(
                "| {} | {:.4} | {:.4} | {} | {:.3} |\n",
                s.fixture_id, s.wer, s.cer, s.silence_false_positive, s.repetition_ratio
            ));
        }
        out.push('\n');
        out
    }
}

fn scenario_group_means(scores: &[ObservatoryFixtureScore]) -> BTreeMap<String, f64> {
    let mut sums: BTreeMap<String, (f64, u32)> = BTreeMap::new();
    for s in scores {
        // Primary scenario tag = first non-meta tag, or "untagged".
        let scenario = s
            .tags
            .iter()
            .find(|t| {
                let l = t.to_ascii_lowercase();
                !matches!(l.as_str(), "synthetic" | "placeholder" | "redistributable")
            })
            .cloned()
            .unwrap_or_else(|| "untagged".into());
        let e = sums.entry(scenario).or_insert((0.0, 0));
        e.0 += s.wer;
        e.1 += 1;
        // Also bucket by each accent_* / silence / long_form tag.
        for t in &s.tags {
            let l = t.to_ascii_lowercase();
            if l.starts_with("accent_")
                || l == "silence"
                || l == "long_form"
                || l == "noisy"
                || l == "lecture"
            {
                let e = sums.entry(l).or_insert((0.0, 0));
                e.0 += s.wer;
                e.1 += 1;
            }
        }
    }
    sums.into_iter()
        .map(|(k, (sum, n))| (k, sum / n.max(1) as f64))
        .collect()
}

/// Build a fixture score from reference/hypothesis using shared metric helpers.
pub fn score_observatory_fixture(
    fixture: &ObservatoryFixture,
    hypothesis: &str,
    extras: ObservatoryScoreExtras,
) -> ObservatoryFixtureScore {
    use super::{
        char_error_rate, normalize_transcript, repetition_ratio, silence_false_positive,
        word_error_rate,
    };

    let wer = word_error_rate(&fixture.reference, hypothesis);
    let cer = char_error_rate(&fixture.reference, hypothesis);
    let ref_n = normalize_transcript(&fixture.reference)
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .count();
    let hyp_n = normalize_transcript(hypothesis)
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .count();
    let length_ratio = hyp_n as f64 / ref_n.max(1) as f64;

    ObservatoryFixtureScore {
        fixture_id: fixture.id.clone(),
        wer,
        cer,
        empty_hypothesis: hypothesis.trim().is_empty(),
        silence_false_positive: silence_false_positive(&fixture.reference, hypothesis),
        repetition_ratio: repetition_ratio(hypothesis),
        length_ratio,
        ref_words: ref_n,
        hyp_words: hyp_n,
        tags: fixture.tags.clone(),
        timestamp_mae_secs: extras.timestamp_mae_secs,
        boundary_error: extras.boundary_error,
        processing_secs: extras.processing_secs,
        rtf: extras.rtf,
    }
}

/// Optional metric fields supplied by the runner.
#[derive(Debug, Clone, Default)]
pub struct ObservatoryScoreExtras {
    pub timestamp_mae_secs: Option<f64>,
    pub boundary_error: Option<f64>,
    pub processing_secs: Option<f64>,
    pub rtf: Option<f64>,
}

// ---------------------------------------------------------------------------
// Budget comparison
// ---------------------------------------------------------------------------

/// Committed baseline budget for one model (or remote lane).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SttBudget {
    pub schema_version: u32,
    pub evidence_version: String,
    pub model: String,
    pub backend_kind: String,
    /// Aggregate mean WER baseline.
    pub baseline_mean_wer: f64,
    /// Absolute WER points allowed beyond relative rule (default 1.0).
    #[serde(default = "default_abs_wer_points")]
    pub max_absolute_wer_points: f64,
    /// Relative WER regression fraction (default 0.10 = 10%).
    #[serde(default = "default_rel_wer")]
    pub max_relative_wer: f64,
    /// Scenario group → baseline mean WER.
    #[serde(default)]
    pub scenario_baseline_wer: BTreeMap<String, f64>,
    /// Relative scenario regression fraction (default 0.15).
    #[serde(default = "default_scenario_rel")]
    pub max_scenario_relative_wer: f64,
    /// Maximum allowed silence false positives on the protected silence set.
    #[serde(default)]
    pub max_silence_false_positives: u32,
    /// Maximum mean repetition ratio.
    #[serde(default = "default_max_rep")]
    pub max_mean_repetition_ratio: f64,
    /// Maximum timestamp MAE (seconds) when backend is marked reliable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_timestamp_mae_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

fn default_abs_wer_points() -> f64 {
    1.0
}
fn default_rel_wer() -> f64 {
    0.10
}
fn default_scenario_rel() -> f64 {
    0.15
}
fn default_max_rep() -> f64 {
    0.35
}

impl SttBudget {
    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read STT budget {}: {e}", path.display()),
        })?;
        serde_json::from_str(&data).map_err(|e| {
            UserError::Other {
                message: format!("parse STT budget: {e}"),
            }
            .into()
        })
    }
}

/// One comparison finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetFinding {
    pub severity: BudgetSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetSeverity {
    Pass,
    Warn,
    Fail,
}

/// Result of comparing a candidate report to a committed budget.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetComparison {
    pub model: String,
    pub passed: bool,
    pub findings: Vec<BudgetFinding>,
    pub candidate_mean_wer: f64,
    pub baseline_mean_wer: f64,
    pub allowed_mean_wer: f64,
}

/// Allowed aggregate mean WER: max(baseline * (1+rel), baseline + abs_points).
pub fn allowed_mean_wer(baseline: f64, max_relative: f64, max_absolute_points: f64) -> f64 {
    let rel = baseline * (1.0 + max_relative);
    let abs = baseline + max_absolute_points;
    rel.max(abs)
}

/// Compare candidate report against budget. Fail-closed on violations.
pub fn compare_stt_budget(report: &ObservatoryReport, budget: &SttBudget) -> BudgetComparison {
    let mut findings = Vec::new();
    let allowed = allowed_mean_wer(
        budget.baseline_mean_wer,
        budget.max_relative_wer,
        budget.max_absolute_wer_points,
    );

    if report.model != budget.model {
        findings.push(BudgetFinding {
            severity: BudgetSeverity::Fail,
            code: "model_mismatch".into(),
            message: format!(
                "report model '{}' != budget model '{}'",
                report.model, budget.model
            ),
        });
    }

    if report.mean_wer > allowed + f64::EPSILON {
        findings.push(BudgetFinding {
            severity: BudgetSeverity::Fail,
            code: "aggregate_wer_regression".into(),
            message: format!(
                "mean WER {:.4} exceeds allowed {:.4} (baseline {:.4}, max(rel {:.0}%, abs +{:.1}))",
                report.mean_wer,
                allowed,
                budget.baseline_mean_wer,
                budget.max_relative_wer * 100.0,
                budget.max_absolute_wer_points
            ),
        });
    }

    if report.silence_false_positives > budget.max_silence_false_positives {
        findings.push(BudgetFinding {
            severity: BudgetSeverity::Fail,
            code: "silence_false_positive".into(),
            message: format!(
                "silence FP {} exceeds max {}",
                report.silence_false_positives, budget.max_silence_false_positives
            ),
        });
    }

    if report.mean_repetition_ratio > budget.max_mean_repetition_ratio + f64::EPSILON {
        findings.push(BudgetFinding {
            severity: BudgetSeverity::Fail,
            code: "repetition_degeneration".into(),
            message: format!(
                "mean repetition {:.4} exceeds max {:.4}",
                report.mean_repetition_ratio, budget.max_mean_repetition_ratio
            ),
        });
    }

    for (scenario, baseline) in &budget.scenario_baseline_wer {
        if let Some(&cand) = report.scenario_mean_wer.get(scenario) {
            let scen_allowed = baseline * (1.0 + budget.max_scenario_relative_wer);
            if cand > scen_allowed + f64::EPSILON {
                findings.push(BudgetFinding {
                    severity: BudgetSeverity::Fail,
                    code: "scenario_wer_regression".into(),
                    message: format!(
                        "scenario '{scenario}' mean WER {cand:.4} exceeds allowed {scen_allowed:.4} (baseline {baseline:.4})"
                    ),
                });
            }
        }
    }

    if let Some(max_mae) = budget.max_timestamp_mae_secs {
        for s in &report.scores {
            if let Some(mae) = s.timestamp_mae_secs {
                if mae > max_mae + f64::EPSILON {
                    findings.push(BudgetFinding {
                        severity: BudgetSeverity::Fail,
                        code: "timestamp_alignment".into(),
                        message: format!(
                            "fixture '{}' timestamp MAE {:.4}s exceeds budget {max_mae:.4}s",
                            s.fixture_id, mae
                        ),
                    });
                }
            }
        }
    }

    if findings.is_empty() {
        findings.push(BudgetFinding {
            severity: BudgetSeverity::Pass,
            code: "ok".into(),
            message: "all budget checks passed".into(),
        });
    }

    let passed = findings.iter().all(|f| f.severity != BudgetSeverity::Fail);
    BudgetComparison {
        model: report.model.clone(),
        passed,
        findings,
        candidate_mean_wer: report.mean_wer,
        baseline_mean_wer: budget.baseline_mean_wer,
        allowed_mean_wer: allowed,
    }
}

/// Exit code helper: 0 pass, 1 fail.
pub fn budget_exit_code(cmp: &BudgetComparison) -> i32 {
    if cmp.passed {
        0
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// Built-in redistributable core (CI-safe)
// ---------------------------------------------------------------------------

/// Small redistributable core corpus for unit tests and CI (not production coverage).
pub fn observatory_core_corpus() -> ObservatoryCorpus {
    ObservatoryCorpus {
        schema_version: OBSERVATORY_SCHEMA_VERSION,
        name: "aurum-observatory-core-v1".into(),
        corpus_version: "observatory-core-v1".into(),
        description: "Redistributable synthetic/control core for schema and budget CI. Full production coverage is the external-fetch pack (see evals/observatory/README.md).".into(),
        fixtures: vec![
            ObservatoryFixture {
                id: "core_clean_en".into(),
                audio: None,
                audio_sha256: None,
                duration_secs: 3.0,
                language: "en".into(),
                reference: "hello world from aurum".into(),
                normalization_policy: NORMALIZATION_POLICY_VERSION.into(),
                tags: vec!["clean".into(), "conversational".into(), "short".into()],
                speaker_id: Some("spk_core_01".into()),
                license: "synthetic CC0".into(),
                provenance: "aurum synthetic text".into(),
                asset_resolution: AssetResolution::Redistributable,
                redistributable: true,
                use_restrictions: None,
                timestamps_expected_reliable: true,
                timing_reference: None,
            },
            ObservatoryFixture {
                id: "core_numbers_en".into(),
                audio: None,
                audio_sha256: None,
                duration_secs: 4.0,
                language: "en".into(),
                reference: "the meeting is at 3 30 pm on 12 january 2026".into(),
                normalization_policy: NORMALIZATION_POLICY_VERSION.into(),
                tags: vec!["numbers".into(), "dates".into(), "clean".into()],
                speaker_id: Some("spk_core_02".into()),
                license: "synthetic CC0".into(),
                provenance: "aurum synthetic text".into(),
                asset_resolution: AssetResolution::Redistributable,
                redistributable: true,
                use_restrictions: None,
                timestamps_expected_reliable: true,
                timing_reference: None,
            },
            ObservatoryFixture {
                id: "core_silence".into(),
                audio: Some("audio/silence_1s.wav".into()),
                audio_sha256: None,
                duration_secs: 1.0,
                language: "en".into(),
                reference: "".into(),
                normalization_policy: NORMALIZATION_POLICY_VERSION.into(),
                tags: vec!["silence".into()],
                speaker_id: None,
                license: "synthetic CC0".into(),
                provenance: "aurum generate_eval_audio".into(),
                asset_resolution: AssetResolution::Redistributable,
                redistributable: true,
                use_restrictions: None,
                timestamps_expected_reliable: true,
                timing_reference: None,
            },
            ObservatoryFixture {
                id: "core_non_speech_tone".into(),
                audio: Some("audio/tone_440_1s.wav".into()),
                audio_sha256: None,
                duration_secs: 1.0,
                language: "en".into(),
                reference: "".into(),
                normalization_policy: NORMALIZATION_POLICY_VERSION.into(),
                tags: vec!["noise".into(), "non_speech".into()],
                speaker_id: None,
                license: "synthetic CC0".into(),
                provenance: "aurum generate_eval_audio".into(),
                asset_resolution: AssetResolution::Redistributable,
                redistributable: true,
                use_restrictions: None,
                timestamps_expected_reliable: true,
                timing_reference: None,
            },
            ObservatoryFixture {
                id: "core_accent_us".into(),
                audio: None,
                audio_sha256: None,
                duration_secs: 5.0,
                language: "en".into(),
                reference: "schedule the call for tomorrow morning".into(),
                normalization_policy: NORMALIZATION_POLICY_VERSION.into(),
                tags: vec!["accent_us".into(), "clean".into()],
                speaker_id: Some("spk_core_03".into()),
                license: "synthetic CC0".into(),
                provenance: "aurum synthetic text".into(),
                asset_resolution: AssetResolution::Redistributable,
                redistributable: true,
                use_restrictions: None,
                timestamps_expected_reliable: true,
                timing_reference: None,
            },
        ],
    }
}

/// Baseline budget for the core corpus perfect-match path (CI negative tests mutate reports).
pub fn observatory_core_budget_tiny() -> SttBudget {
    SttBudget {
        schema_version: OBSERVATORY_SCHEMA_VERSION,
        evidence_version: STT_OBSERVATORY_EVIDENCE_VERSION.into(),
        model: "tiny-q5_1".into(),
        backend_kind: "asr".into(),
        baseline_mean_wer: 0.0,
        max_absolute_wer_points: 1.0,
        max_relative_wer: 0.10,
        scenario_baseline_wer: BTreeMap::from([("silence".into(), 0.0), ("clean".into(), 0.0)]),
        max_scenario_relative_wer: 0.15,
        max_silence_false_positives: 0,
        max_mean_repetition_ratio: 0.35,
        max_timestamp_mae_secs: None,
        notes: Some("Core CI budget for perfect-match synthetic scoring".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_corpus_validates_without_production() {
        let c = observatory_core_corpus();
        let cov = c.validate(false).unwrap();
        assert!(cov.fixture_count >= 5);
        assert!(cov.has_silence);
        assert!(c.validate(true).is_err());
    }

    #[test]
    fn production_coverage_requires_minima() {
        let mut c = observatory_core_corpus();
        // Expand to satisfy production minima with synthetic placeholders.
        c.fixtures.clear();
        for i in 0..25 {
            c.fixtures.push(ObservatoryFixture {
                id: format!("prod_{i:02}"),
                audio: None,
                audio_sha256: None,
                duration_secs: 150.0,
                language: "en".into(),
                reference: "hello".into(),
                normalization_policy: NORMALIZATION_POLICY_VERSION.into(),
                tags: vec![
                    "clean".into(),
                    "conversational".into(),
                    "lecture".into(),
                    "noisy".into(),
                    "numbers".into(),
                    "multilingual".into(),
                    "low_volume".into(),
                    format!("accent_{}", ["us", "gb", "au", "in"][i % 4]),
                    if i < 3 {
                        "long_form".into()
                    } else {
                        "short".into()
                    },
                    if i == 0 {
                        "silence".into()
                    } else {
                        "speech".into()
                    },
                ],
                speaker_id: Some(format!("spk_{i:02}")),
                license: "test".into(),
                provenance: "unit".into(),
                asset_resolution: AssetResolution::ExternalFetch,
                redistributable: false,
                use_restrictions: Some("test only".into()),
                timestamps_expected_reliable: true,
                timing_reference: None,
            });
        }
        // First fixture is silence control with empty reference.
        c.fixtures[0].reference = String::new();
        let cov = c.validate(true).unwrap();
        assert!(cov.total_duration_secs >= 3600.0);
        assert!(cov.speaker_count >= 20);
    }

    #[test]
    fn budget_allows_within_tolerance() {
        let corpus = observatory_core_corpus();
        let scores: Vec<_> = corpus
            .fixtures
            .iter()
            .map(|f| score_observatory_fixture(f, &f.reference, Default::default()))
            .collect();
        let report = ObservatoryReport::from_scores(
            &corpus,
            "tiny-q5_1",
            "asr",
            scores,
            RunIdentity::default(),
        );
        let budget = observatory_core_budget_tiny();
        let cmp = compare_stt_budget(&report, &budget);
        assert!(cmp.passed, "{:?}", cmp.findings);
        assert_eq!(budget_exit_code(&cmp), 0);
    }

    #[test]
    fn budget_fails_on_wer_regression() {
        let corpus = observatory_core_corpus();
        let scores: Vec<_> = corpus
            .fixtures
            .iter()
            .map(|f| {
                // Deliberately garbage hypothesis.
                score_observatory_fixture(f, "zzz yyy xxx www vvv", Default::default())
            })
            .collect();
        let report = ObservatoryReport::from_scores(
            &corpus,
            "tiny-q5_1",
            "asr",
            scores,
            RunIdentity::default(),
        );
        let budget = observatory_core_budget_tiny();
        let cmp = compare_stt_budget(&report, &budget);
        assert!(!cmp.passed);
        assert_eq!(budget_exit_code(&cmp), 1);
        assert!(cmp
            .findings
            .iter()
            .any(|f| f.code == "aggregate_wer_regression"));
    }

    #[test]
    fn budget_fails_on_new_silence_fp() {
        let corpus = observatory_core_corpus();
        let scores: Vec<_> = corpus
            .fixtures
            .iter()
            .map(|f| {
                let hyp = if f.reference.is_empty() {
                    "hallucinated text"
                } else {
                    f.reference.as_str()
                };
                score_observatory_fixture(f, hyp, Default::default())
            })
            .collect();
        let report = ObservatoryReport::from_scores(
            &corpus,
            "tiny-q5_1",
            "asr",
            scores,
            RunIdentity::default(),
        );
        let budget = observatory_core_budget_tiny();
        let cmp = compare_stt_budget(&report, &budget);
        assert!(!cmp.passed);
        assert!(cmp
            .findings
            .iter()
            .any(|f| f.code == "silence_false_positive"));
    }

    #[test]
    fn allowed_wer_uses_larger_of_rel_and_abs() {
        // baseline 0.05: rel 10% → 0.055; abs +1.0 → 1.05 → allowed 1.05
        assert!((allowed_mean_wer(0.05, 0.10, 1.0) - 1.05).abs() < 1e-9);
        // baseline 0.50: rel 10% → 0.55; abs +1.0 → 1.50 → allowed 1.50
        assert!((allowed_mean_wer(0.50, 0.10, 1.0) - 1.50).abs() < 1e-9);
        // baseline 0.40 with abs 0.05: rel → 0.44; abs → 0.45 → 0.45
        assert!((allowed_mean_wer(0.40, 0.10, 0.05) - 0.45).abs() < 1e-9);
    }

    #[test]
    fn scorecard_and_json_deterministic() {
        let corpus = observatory_core_corpus();
        let scores: Vec<_> = corpus
            .fixtures
            .iter()
            .map(|f| score_observatory_fixture(f, &f.reference, Default::default()))
            .collect();
        let r1 = ObservatoryReport::from_scores(
            &corpus,
            "tiny-q5_1",
            "asr",
            scores.clone(),
            RunIdentity::default(),
        );
        let r2 = ObservatoryReport::from_scores(
            &corpus,
            "tiny-q5_1",
            "asr",
            scores,
            RunIdentity::default(),
        );
        assert_eq!(r1.to_json_pretty().unwrap(), r2.to_json_pretty().unwrap());
        let md = r1.to_markdown_scorecard();
        assert!(md.contains("mean WER"));
        assert!(md.contains("core_clean_en"));
    }

    #[test]
    fn duplicate_fixture_id_rejected() {
        let mut c = observatory_core_corpus();
        c.fixtures.push(c.fixtures[0].clone());
        assert!(c.validate(false).is_err());
    }
}
