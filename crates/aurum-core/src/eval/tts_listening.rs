//! Production TTS listening, pronunciation and join-quality evidence (JOE-2217).
//!
//! Objective PCM metrics are deterministic and CI-safe. Human listening protocol
//! types capture aggregates only (no listener PII). Support-tier promotion rules
//! cite both objective and listening evidence.

use crate::error::{Result, UserError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// TTS evaluation pack schema version.
pub const TTS_EVAL_SCHEMA_VERSION: u32 = 1;

/// Evidence pack id for TTS objective + listening programme.
pub const TTS_EVIDENCE_VERSION: &str = "0.0.22-tts-listening-v1";

// ---------------------------------------------------------------------------
// Fixture pack
// ---------------------------------------------------------------------------

/// Whether a fixture participates in human listening, objective checks, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsEvalParticipation {
    ObjectiveOnly,
    ListeningOnly,
    Both,
}

/// One TTS evaluation utterance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsEvalFixture {
    pub id: String,
    pub text: String,
    /// BCP-47-ish language tag (e.g. `en-US`, `en-GB`).
    pub language: String,
    /// Category tags: short, numbers, currency, dates, abbrev, proper_noun,
    /// homograph, question, exclamation, quote, join, long_form, chunk_boundary,
    /// control, …
    #[serde(default)]
    pub tags: Vec<String>,
    /// Reviewer notes for expected pronunciation (not scored automatically).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pronunciation_notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms_min: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms_max: Option<u64>,
    #[serde(default)]
    pub license: String,
    #[serde(default = "default_both")]
    pub participation: TtsEvalParticipation,
}

fn default_both() -> TtsEvalParticipation {
    TtsEvalParticipation::Both
}

/// Versioned TTS evaluation pack (≥60 utterances for production).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsEvalPack {
    pub schema_version: u32,
    pub name: String,
    pub pack_version: String,
    #[serde(default)]
    pub description: String,
    pub fixtures: Vec<TtsEvalFixture>,
}

impl TtsEvalPack {
    pub fn validate(&self, enforce_min_count: bool) -> Result<()> {
        if self.schema_version != TTS_EVAL_SCHEMA_VERSION {
            return Err(UserError::Other {
                message: format!(
                    "unsupported TTS eval schema_version {} (expected {TTS_EVAL_SCHEMA_VERSION})",
                    self.schema_version
                ),
            }
            .into());
        }
        if self.fixtures.is_empty() {
            return Err(UserError::Other {
                message: "TTS eval pack has no fixtures".into(),
            }
            .into());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut cats = std::collections::BTreeSet::new();
        for f in &self.fixtures {
            if f.id.trim().is_empty() {
                return Err(UserError::Other {
                    message: "TTS fixture id must be non-empty".into(),
                }
                .into());
            }
            if !ids.insert(f.id.clone()) {
                return Err(UserError::Other {
                    message: format!("duplicate TTS fixture id '{}'", f.id),
                }
                .into());
            }
            if f.text.len() > 100_000 {
                return Err(UserError::Other {
                    message: format!("fixture '{}' text exceeds size bound", f.id),
                }
                .into());
            }
            for t in &f.tags {
                cats.insert(t.to_ascii_lowercase());
            }
        }
        if enforce_min_count && self.fixtures.len() < 60 {
            return Err(UserError::Other {
                message: format!(
                    "TTS eval pack has {} fixtures (minimum 60)",
                    self.fixtures.len()
                ),
            }
            .into());
        }
        if enforce_min_count {
            let required = [
                "short",
                "numbers",
                "currency",
                "dates",
                "abbrev",
                "proper_noun",
                "homograph",
                "question",
                "exclamation",
                "quote",
                "join",
                "long_form",
                "chunk_boundary",
            ];
            let mut missing = Vec::new();
            for r in required {
                if !cats.iter().any(|c| c == r) {
                    missing.push(r);
                }
            }
            if !missing.is_empty() {
                return Err(UserError::Other {
                    message: format!("TTS pack missing category tags: {}", missing.join(", ")),
                }
                .into());
            }
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read TTS eval pack {}: {e}", path.display()),
        })?;
        if data.len() > 16 * 1024 * 1024 {
            return Err(UserError::Other {
                message: "TTS eval pack exceeds 16 MiB size bound".into(),
            }
            .into());
        }
        serde_json::from_str(&data).map_err(|e| {
            UserError::Other {
                message: format!("parse TTS eval pack: {e}"),
            }
            .into()
        })
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            UserError::Other {
                message: format!("serialize TTS eval pack: {e}"),
            }
            .into()
        })
    }
}

// ---------------------------------------------------------------------------
// Objective PCM metrics
// ---------------------------------------------------------------------------

/// Objective synthesis metrics for one model/voice × fixture (no payload text).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsObjectiveScore {
    pub fixture_id: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub sample_count: usize,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtf: Option<f64>,
    #[serde(default)]
    pub chunk_count: u32,
    #[serde(default)]
    pub char_count: usize,
    pub peak_amplitude: f32,
    pub rms: f32,
    pub clipped_samples: u64,
    pub leading_silence_ms: u64,
    pub trailing_silence_ms: u64,
    /// Mean absolute first-difference at declared join boundaries (0 = smooth).
    #[serde(default)]
    pub join_discontinuity: f64,
    pub empty_or_near_empty: bool,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcm_sha256: Option<String>,
    pub passed: bool,
    #[serde(default)]
    pub failures: Vec<String>,
}

/// Run identity for a TTS objective report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TtsRunIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aurum_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_profile: Option<String>,
}

/// Versioned objective TTS report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsObjectiveReport {
    pub schema_version: u32,
    pub evidence_version: String,
    pub pack_name: String,
    pub pack_version: String,
    pub model: String,
    pub voice: String,
    #[serde(default)]
    pub identity: TtsRunIdentity,
    pub scores: Vec<TtsObjectiveScore>,
    pub passed_count: u32,
    pub failed_count: u32,
    pub all_passed: bool,
}

impl TtsObjectiveReport {
    pub fn from_scores(
        pack: &TtsEvalPack,
        model: &str,
        voice: &str,
        mut scores: Vec<TtsObjectiveScore>,
        identity: TtsRunIdentity,
    ) -> Self {
        scores.sort_by(|a, b| a.fixture_id.cmp(&b.fixture_id));
        let passed_count = scores.iter().filter(|s| s.passed).count() as u32;
        let failed_count = scores.len() as u32 - passed_count;
        Self {
            schema_version: TTS_EVAL_SCHEMA_VERSION,
            evidence_version: TTS_EVIDENCE_VERSION.into(),
            pack_name: pack.name.clone(),
            pack_version: pack.pack_version.clone(),
            model: model.into(),
            voice: voice.into(),
            identity,
            scores,
            passed_count,
            failed_count,
            all_passed: failed_count == 0,
        }
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            UserError::Other {
                message: format!("serialize TTS objective report: {e}"),
            }
            .into()
        })
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# TTS objective scorecard\n\n");
        out.push_str(&format!(
            "- **Evidence:** {}\n- **Pack:** {} ({})\n- **Model/voice:** `{}` / `{}`\n- **Passed:** {}/{}\n\n",
            self.evidence_version,
            self.pack_name,
            self.pack_version,
            self.model,
            self.voice,
            self.passed_count,
            self.passed_count + self.failed_count
        ));
        out.push_str("| Fixture | dur_ms | peak | clip | join | empty | pass |\n");
        out.push_str("|---------|--------|------|------|------|-------|------|\n");
        for s in &self.scores {
            out.push_str(&format!(
                "| {} | {} | {:.3} | {} | {:.4} | {} | {} |\n",
                s.fixture_id,
                s.duration_ms,
                s.peak_amplitude,
                s.clipped_samples,
                s.join_discontinuity,
                s.empty_or_near_empty,
                s.passed
            ));
        }
        out.push('\n');
        out
    }
}

/// Thresholds for objective pass/fail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsObjectiveThresholds {
    /// Peak amplitude above this counts as hard-clip risk (float full scale).
    pub max_peak: f32,
    /// Samples with |s| >= this count as clipped.
    pub clip_threshold: f32,
    /// Max allowed clipped samples.
    pub max_clipped_samples: u64,
    /// Near-empty if duration below this (ms) for non-control fixtures.
    pub min_duration_ms: u64,
    /// Join discontinuity above this fails.
    pub max_join_discontinuity: f64,
}

impl Default for TtsObjectiveThresholds {
    fn default() -> Self {
        Self {
            max_peak: 1.0,
            clip_threshold: 0.999,
            max_clipped_samples: 0,
            min_duration_ms: 50,
            max_join_discontinuity: 0.85,
        }
    }
}

/// Compute objective metrics from mono f32 PCM in [-1, 1].
///
/// `join_indices` are sample indices of chunk boundaries (may be empty).
#[allow(clippy::too_many_arguments)] // runner-facing metric surface; args are independent dimensions
pub fn score_tts_pcm(
    fixture: &TtsEvalFixture,
    samples: &[f32],
    sample_rate_hz: u32,
    channels: u16,
    chunk_count: u32,
    join_indices: &[usize],
    wall_ms: Option<u64>,
    thresholds: &TtsObjectiveThresholds,
    truncated: bool,
) -> TtsObjectiveScore {
    let mut failures = Vec::new();
    if sample_rate_hz == 0 {
        failures.push("invalid_sample_rate".into());
    }
    let sample_count = samples.len();
    let duration_ms = if sample_rate_hz > 0 {
        (sample_count as u64 * 1000) / sample_rate_hz as u64
    } else {
        0
    };

    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    let mut clipped = 0u64;
    for &s in samples {
        if !s.is_finite() {
            failures.push("non_finite_sample".into());
            break;
        }
        let a = s.abs();
        peak = peak.max(a);
        sum_sq += (s as f64) * (s as f64);
        if a >= thresholds.clip_threshold {
            clipped += 1;
        }
    }
    let rms = if sample_count == 0 {
        0.0
    } else {
        (sum_sq / sample_count as f64).sqrt() as f32
    };

    let silence_thresh = 1e-4f32;
    let leading = count_edge_silence_ms(samples, sample_rate_hz, silence_thresh, true);
    let trailing = count_edge_silence_ms(samples, sample_rate_hz, silence_thresh, false);
    let join_discontinuity = join_discontinuity_score(samples, join_indices);

    let empty_or_near_empty = sample_count < 16 || duration_ms < thresholds.min_duration_ms;
    let is_control = fixture.tags.iter().any(|t| {
        let l = t.to_ascii_lowercase();
        l == "control" || l == "invalid_input"
    });

    if !is_control && empty_or_near_empty {
        failures.push("empty_or_near_empty".into());
    }
    if peak > thresholds.max_peak + f32::EPSILON {
        failures.push(format!("peak_exceeds_{}", thresholds.max_peak));
    }
    if clipped > thresholds.max_clipped_samples {
        failures.push(format!("clipped_samples_{clipped}"));
    }
    if join_discontinuity > thresholds.max_join_discontinuity + f64::EPSILON {
        failures.push(format!("join_discontinuity_{join_discontinuity:.4}"));
    }
    if truncated {
        failures.push("truncated".into());
    }
    if let Some(min) = fixture.duration_ms_min {
        if duration_ms < min && !is_control {
            failures.push(format!("duration_below_min_{min}"));
        }
    }
    if let Some(max) = fixture.duration_ms_max {
        if duration_ms > max {
            failures.push(format!("duration_above_max_{max}"));
        }
    }

    let rtf = wall_ms.map(|w| {
        if duration_ms == 0 {
            0.0
        } else {
            w as f64 / duration_ms as f64
        }
    });

    TtsObjectiveScore {
        fixture_id: fixture.id.clone(),
        sample_rate_hz,
        channels,
        sample_count,
        duration_ms,
        wall_ms,
        rtf,
        chunk_count,
        char_count: fixture.text.chars().count(),
        peak_amplitude: peak,
        rms,
        clipped_samples: clipped,
        leading_silence_ms: leading,
        trailing_silence_ms: trailing,
        join_discontinuity,
        empty_or_near_empty,
        truncated,
        pcm_sha256: None,
        passed: failures.is_empty(),
        failures,
    }
}

fn count_edge_silence_ms(samples: &[f32], sr: u32, thresh: f32, leading: bool) -> u64 {
    if samples.is_empty() || sr == 0 {
        return 0;
    }
    let mut n = 0usize;
    if leading {
        for &s in samples {
            if s.abs() < thresh {
                n += 1;
            } else {
                break;
            }
        }
    } else {
        for &s in samples.iter().rev() {
            if s.abs() < thresh {
                n += 1;
            } else {
                break;
            }
        }
    }
    (n as u64 * 1000) / sr as u64
}

/// Mean absolute step at join indices (normalized roughly to [0, 2]).
pub fn join_discontinuity_score(samples: &[f32], join_indices: &[usize]) -> f64 {
    if samples.len() < 2 || join_indices.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut n = 0u32;
    for &idx in join_indices {
        if idx > 0 && idx < samples.len() {
            sum += (samples[idx] as f64 - samples[idx - 1] as f64).abs();
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

// ---------------------------------------------------------------------------
// Human listening protocol (aggregates only)
// ---------------------------------------------------------------------------

/// One blinded rating (1–5) for a fixture × model label.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListeningRating {
    pub fixture_id: String,
    /// Opaque randomized model label (not the real model name at capture time).
    pub blinded_label: String,
    pub intelligibility: u8,
    pub naturalness: u8,
    pub pronunciation: u8,
    pub join_smoothness: u8,
    #[serde(default)]
    pub critical_failure: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Aggregate listening report (no listener PII).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListeningReport {
    pub schema_version: u32,
    pub evidence_version: String,
    pub round_id: String,
    pub protocol_version: String,
    pub listener_count: u32,
    pub blinding: bool,
    pub playback_normalization: String,
    /// Map of real model id → aggregate medians.
    pub model_aggregates: BTreeMap<String, ListeningAggregate>,
    pub critical_failure_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListeningAggregate {
    pub n_ratings: u32,
    pub median_intelligibility: f64,
    pub median_naturalness: f64,
    pub median_pronunciation: f64,
    pub median_join_smoothness: f64,
    pub critical_failures: u32,
}

/// Compute medians from raw ratings (host maps blinded labels → model ids first).
pub fn aggregate_listening(
    by_model: &BTreeMap<String, Vec<ListeningRating>>,
) -> BTreeMap<String, ListeningAggregate> {
    let mut out = BTreeMap::new();
    for (model, ratings) in by_model {
        if ratings.is_empty() {
            continue;
        }
        let mut intel: Vec<u8> = ratings.iter().map(|r| r.intelligibility).collect();
        let mut nat: Vec<u8> = ratings.iter().map(|r| r.naturalness).collect();
        let mut pro: Vec<u8> = ratings.iter().map(|r| r.pronunciation).collect();
        let mut join: Vec<u8> = ratings.iter().map(|r| r.join_smoothness).collect();
        let critical = ratings.iter().filter(|r| r.critical_failure).count() as u32;
        out.insert(
            model.clone(),
            ListeningAggregate {
                n_ratings: ratings.len() as u32,
                median_intelligibility: median_u8(&mut intel),
                median_naturalness: median_u8(&mut nat),
                median_pronunciation: median_u8(&mut pro),
                median_join_smoothness: median_u8(&mut join),
                critical_failures: critical,
            },
        );
    }
    out
}

fn median_u8(v: &mut [u8]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2] as f64
    } else {
        (v[n / 2 - 1] as f64 + v[n / 2] as f64) / 2.0
    }
}

/// Support-tier decision from objective + listening aggregates (JOE-2217 policy).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupportTierDecision {
    pub model: String,
    pub voice: String,
    pub supported: bool,
    pub reasons: Vec<String>,
}

/// Local model may be documented as supported only when:
/// * all objective safety/correctness checks pass;
/// * no critical omission/truncation in the golden set;
/// * median intelligibility ≥ 4/5;
/// * median pronunciation and join smoothness ≥ 3.5/5.
pub fn evaluate_support_tier(
    model: &str,
    voice: &str,
    objective_all_passed: bool,
    listening: Option<&ListeningAggregate>,
    known_limitations_documented: bool,
) -> SupportTierDecision {
    let mut reasons = Vec::new();
    let mut supported = true;
    if !objective_all_passed {
        supported = false;
        reasons.push("objective checks failed".into());
    }
    match listening {
        None => {
            supported = false;
            reasons
                .push("missing listening aggregate (≥3 listeners required for promotion)".into());
        }
        Some(a) => {
            if a.critical_failures > 0 {
                supported = false;
                reasons.push(format!(
                    "{} critical listening failures",
                    a.critical_failures
                ));
            }
            if a.median_intelligibility < 4.0 {
                supported = false;
                reasons.push(format!(
                    "median intelligibility {:.1} < 4.0",
                    a.median_intelligibility
                ));
            }
            if a.median_pronunciation < 3.5 {
                supported = false;
                reasons.push(format!(
                    "median pronunciation {:.1} < 3.5",
                    a.median_pronunciation
                ));
            }
            if a.median_join_smoothness < 3.5 {
                supported = false;
                reasons.push(format!(
                    "median join smoothness {:.1} < 3.5",
                    a.median_join_smoothness
                ));
            }
        }
    }
    if !known_limitations_documented {
        supported = false;
        reasons.push("known limitations not documented".into());
    }
    if supported {
        reasons.push("meets objective + listening support policy".into());
    }
    SupportTierDecision {
        model: model.into(),
        voice: voice.into(),
        supported,
        reasons,
    }
}

// ---------------------------------------------------------------------------
// Built-in production fixture pack (≥60)
// ---------------------------------------------------------------------------

fn fix(id: &str, text: &str, language: &str, tags: &[&str], notes: Option<&str>) -> TtsEvalFixture {
    TtsEvalFixture {
        id: id.into(),
        text: text.into(),
        language: language.into(),
        tags: tags.iter().map(|s| (*s).into()).collect(),
        pronunciation_notes: notes.map(|s| s.into()),
        duration_ms_min: None,
        duration_ms_max: None,
        license: "synthetic CC0".into(),
        participation: TtsEvalParticipation::Both,
    }
}

/// Production TTS evaluation pack with ≥60 utterances covering JOE-2217 categories.
pub fn tts_production_pack() -> TtsEvalPack {
    let mut fixtures = Vec::new();

    // Short conversational (10)
    let shorts = [
        ("tts_s01", "Hello from Aurum.", "short"),
        ("tts_s02", "Thanks for calling.", "short"),
        ("tts_s03", "See you tomorrow.", "short"),
        ("tts_s04", "Please hold on a moment.", "short"),
        ("tts_s05", "I can help with that.", "short"),
        ("tts_s06", "Good morning, everyone.", "short"),
        ("tts_s07", "Let's get started.", "short"),
        ("tts_s08", "That sounds good to me.", "short"),
        ("tts_s09", "I'll send the notes shortly.", "short"),
        ("tts_s10", "Talk soon.", "short"),
    ];
    for (id, text, tag) in shorts {
        fixtures.push(fix(id, text, "en-US", &[tag, "conversational"], None));
    }

    // Punctuation / cadence (5)
    fixtures.push(fix(
        "tts_p01",
        "First, we plan. Second, we build. Third, we ship.",
        "en-US",
        &["punctuation", "cadence", "short"],
        None,
    ));
    fixtures.push(fix(
        "tts_p02",
        "Wait — did you mean the blue one, or the green one?",
        "en-US",
        &["punctuation", "question"],
        None,
    ));
    fixtures.push(fix(
        "tts_p03",
        "Yes! Absolutely. We should do that.",
        "en-US",
        &["punctuation", "exclamation"],
        None,
    ));
    fixtures.push(fix(
        "tts_p04",
        "She said, \"Meet me at noon,\" and left.",
        "en-US",
        &["quote", "punctuation"],
        None,
    ));
    fixtures.push(fix(
        "tts_p05",
        "Email me at support at example dot com, please.",
        "en-US",
        &["punctuation", "abbrev"],
        None,
    ));

    // Numbers, currency, dates, times, measurements (10)
    fixtures.push(fix(
        "tts_n01",
        "Call me at extension 42.",
        "en-US",
        &["numbers", "abbrev"],
        Some("extension forty-two"),
    ));
    fixtures.push(fix(
        "tts_n02",
        "The total is $1,234.56.",
        "en-US",
        &["currency", "numbers"],
        Some("one thousand two hundred thirty-four dollars and fifty-six cents"),
    ));
    fixtures.push(fix(
        "tts_n03",
        "The meeting is on January 12, 2026 at 3:30 p.m.",
        "en-US",
        &["dates", "times", "numbers"],
        None,
    ));
    fixtures.push(fix(
        "tts_n04",
        "Drive 15.5 kilometres north on Route 66.",
        "en-US",
        &["measurements", "numbers"],
        None,
    ));
    fixtures.push(fix(
        "tts_n05",
        "Version 0.0.22 ships next week.",
        "en-US",
        &["numbers", "abbrev"],
        None,
    ));
    fixtures.push(fix(
        "tts_n06",
        "The temperature was -4 degrees Celsius.",
        "en-US",
        &["numbers", "measurements"],
        None,
    ));
    fixtures.push(fix(
        "tts_n07",
        "Order number A-90421 was fulfilled.",
        "en-US",
        &["numbers", "abbrev"],
        None,
    ));
    fixtures.push(fix(
        "tts_n08",
        "We need 3 GB of RAM and 512 MB of cache.",
        "en-US",
        &["numbers", "abbrev", "measurements"],
        None,
    ));
    fixtures.push(fix(
        "tts_n09",
        "Arrive by 09:05 or take the 17:45 train.",
        "en-US",
        &["times", "numbers"],
        None,
    ));
    fixtures.push(fix(
        "tts_n10",
        "Pi is approximately 3.14159.",
        "en-US",
        &["numbers"],
        None,
    ));

    // Abbreviations / initialisms (5)
    fixtures.push(fix(
        "tts_a01",
        "The CPU and GPU metrics for the API gateway.",
        "en-US",
        &["abbrev", "acronym"],
        Some("C-P-U, G-P-U, A-P-I"),
    ));
    fixtures.push(fix(
        "tts_a02",
        "Please CC the CEO and the CTO.",
        "en-US",
        &["abbrev"],
        None,
    ));
    fixtures.push(fix(
        "tts_a03",
        "HTTPS is required for STT and TTS endpoints.",
        "en-US",
        &["abbrev", "acronym"],
        None,
    ));
    fixtures.push(fix(
        "tts_a04",
        "The FAQ covers RTF and WER definitions.",
        "en-US",
        &["abbrev", "acronym"],
        None,
    ));
    fixtures.push(fix(
        "tts_a05",
        "Use SHA-256 digests, not MD5.",
        "en-US",
        &["abbrev", "acronym"],
        None,
    ));

    // Proper nouns / difficult graphemes (5)
    fixtures.push(fix(
        "tts_pn01",
        "Aurum transcribes speech on-device by default.",
        "en-US",
        &["proper_noun"],
        Some("Aurum as product name"),
    ));
    fixtures.push(fix(
        "tts_pn02",
        "Dr. Nguyen met Mr. O'Brien in Edinburgh.",
        "en-US",
        &["proper_noun"],
        None,
    ));
    fixtures.push(fix(
        "tts_pn03",
        "Worcestershire sauce is hard to spell.",
        "en-US",
        &["proper_noun", "difficult"],
        Some("WUSS-ter-sheer"),
    ));
    fixtures.push(fix(
        "tts_pn04",
        "The queue at the boutique was irregular.",
        "en-US",
        &["difficult"],
        None,
    ));
    fixtures.push(fix(
        "tts_pn05",
        "Pho and banh mi were on the menu.",
        "en-US",
        &["proper_noun", "difficult"],
        None,
    ));

    // Homographs (4)
    fixtures.push(fix(
        "tts_h01",
        "I will record the record after the meeting.",
        "en-US",
        &["homograph"],
        Some("re-CORD vs REC-ord"),
    ));
    fixtures.push(fix(
        "tts_h02",
        "Please present the present to the guest.",
        "en-US",
        &["homograph"],
        None,
    ));
    fixtures.push(fix(
        "tts_h03",
        "The wind will wind around the tower.",
        "en-US",
        &["homograph"],
        None,
    ));
    fixtures.push(fix(
        "tts_h04",
        "They live near the live venue.",
        "en-US",
        &["homograph"],
        None,
    ));

    // Questions / exclamations (4)
    fixtures.push(fix(
        "tts_q01",
        "What time does the train leave?",
        "en-US",
        &["question"],
        None,
    ));
    fixtures.push(fix(
        "tts_q02",
        "Could you repeat that, please?",
        "en-US",
        &["question"],
        None,
    ));
    fixtures.push(fix(
        "tts_e01",
        "Watch out for the ice!",
        "en-US",
        &["exclamation"],
        None,
    ));
    fixtures.push(fix(
        "tts_e02",
        "Congratulations on the release!",
        "en-US",
        &["exclamation"],
        None,
    ));

    // US / UK (4)
    fixtures.push(fix(
        "tts_uk01",
        "Please organise the colour catalogue for the theatre.",
        "en-GB",
        &["en_gb", "short"],
        Some("UK spelling preferences"),
    ));
    fixtures.push(fix(
        "tts_uk02",
        "The lift is opposite the flat near the queue.",
        "en-GB",
        &["en_gb", "short"],
        None,
    ));
    fixtures.push(fix(
        "tts_us01",
        "Please organize the color catalog for the theater.",
        "en-US",
        &["en_us", "short"],
        None,
    ));
    fixtures.push(fix(
        "tts_us02",
        "The elevator is across from the apartment near the line.",
        "en-US",
        &["en_us", "short"],
        None,
    ));

    // Chunk boundary / join stress (4)
    fixtures.push(fix(
        "tts_j01",
        "First sentence ends here. Second sentence starts now and continues a little longer.",
        "en-US",
        &["join", "chunk_boundary"],
        None,
    ));
    fixtures.push(fix(
        "tts_j02",
        "Alpha. Bravo. Charlie. Delta. Echo. Foxtrot.",
        "en-US",
        &["join", "chunk_boundary"],
        None,
    ));
    fixtures.push(fix(
        "tts_j03",
        "One two three four five six seven eight nine ten eleven twelve.",
        "en-US",
        &["join", "numbers"],
        None,
    ));
    fixtures.push(fix(
        "tts_j04",
        "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs.",
        "en-US",
        &["join", "chunk_boundary"],
        None,
    ));

    // Long-form multi-paragraph (3)
    fixtures.push(fix(
        "tts_lf01",
        "Welcome to the weekly product update. Today we cover shipping, quality, and support. First, shipping: the release candidate is green on all Tier A platforms. Second, quality: the observatory budgets are checked in and fail closed. Third, support: please file issues with redacted diagnostics only.",
        "en-US",
        &["long_form", "join"],
        None,
    ));
    fixtures.push(fix(
        "tts_lf02",
        "Chapter one begins with a quiet morning. Birds called from the trees, and the river moved slowly past the mill. In the village square, merchants opened their stalls. A child asked for a story, and an old traveller began to speak.",
        "en-US",
        &["long_form", "join"],
        None,
    ));
    fixtures.push(fix(
        "tts_lf03",
        "Operator checklist: verify model digests, confirm local-only policy, run doctor, execute the smoke corpus, compare budgets, and only then publish. Do not skip integrity checks. Do not paste API keys into tickets. Prefer relative paths in public support bundles.",
        "en-US",
        &["long_form"],
        None,
    ));

    // Very short + near chunk-size edge (3)
    fixtures.push(fix(
        "tts_v01",
        "Hi.",
        "en-US",
        &["short", "very_short"],
        None,
    ));
    fixtures.push(fix(
        "tts_v02",
        "OK.",
        "en-US",
        &["short", "very_short"],
        None,
    ));
    fixtures.push(fix(
        "tts_v03",
        "Yes.",
        "en-US",
        &["short", "very_short"],
        None,
    ));

    // Controls (3) — objective only
    let mut ctrl = fix(
        "tts_c01",
        "",
        "en-US",
        &["control", "invalid_input"],
        Some("empty text control"),
    );
    ctrl.participation = TtsEvalParticipation::ObjectiveOnly;
    fixtures.push(ctrl);
    let mut ctrl2 = fix(
        "tts_c02",
        "   ",
        "en-US",
        &["control", "invalid_input"],
        Some("whitespace-only control"),
    );
    ctrl2.participation = TtsEvalParticipation::ObjectiveOnly;
    fixtures.push(ctrl2);
    fixtures.push(fix(
        "tts_c03",
        "Silence after this period. ",
        "en-US",
        &["control", "trailing"],
        None,
    ));

    // Extra coverage to clear 60+
    fixtures.push(fix(
        "tts_x01",
        "The FAQ, README, and CHANGELOG must stay aligned.",
        "en-US",
        &["abbrev", "proper_noun"],
        None,
    ));
    fixtures.push(fix(
        "tts_x02",
        "Is the p95 under 200 milliseconds?",
        "en-US",
        &["question", "numbers", "measurements"],
        None,
    ));
    fixtures.push(fix(
        "tts_x03",
        "Stop! Do not overwrite the baseline without review.",
        "en-US",
        &["exclamation"],
        None,
    ));
    fixtures.push(fix(
        "tts_x04",
        "He read the red book by the reed bed.",
        "en-US",
        &["homograph", "difficult"],
        None,
    ));
    fixtures.push(fix(
        "tts_x05",
        "Schedule a call for 2nd February at half past four.",
        "en-GB",
        &["dates", "times", "en_gb"],
        None,
    ));

    debug_assert!(fixtures.len() >= 60, "pack must have ≥60 fixtures");

    TtsEvalPack {
        schema_version: TTS_EVAL_SCHEMA_VERSION,
        name: "aurum-tts-eval-v1".into(),
        pack_version: "tts-eval-v1".into(),
        description: "Production TTS evaluation pack: pronunciation, prosody, joins, and controls (JOE-2217). Synthetic CC0 text; no private synthesis payloads.".into(),
        fixtures,
    }
}

/// Recommended local matrix for objective runs.
pub fn tts_local_matrix() -> Vec<(&'static str, &'static str, &'static str)> {
    // (model, voice, role)
    vec![
        ("kitten-nano-int8", "Luna", "default"),
        ("kitten-nano-int8", "male", "additional_male"),
        ("kitten-nano-int8", "female", "additional_female"),
        ("kokoro-82m-int8", "default", "default"),
        ("kokoro-82m-int8", "af_sarah", "us"),
        ("kokoro-82m-int8", "bf_emma", "uk"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_pack_meets_minimums() {
        let p = tts_production_pack();
        assert!(p.fixtures.len() >= 60, "got {}", p.fixtures.len());
        p.validate(true).unwrap();
        let json = p.to_json_pretty().unwrap();
        assert!(json.contains("tts_s01"));
        assert!(json.contains("homograph"));
    }

    #[test]
    fn objective_rejects_clipped_and_empty() {
        let f = fix("t", "hello", "en-US", &["short"], None);
        let thr = TtsObjectiveThresholds::default();
        // empty
        let empty = score_tts_pcm(&f, &[], 24_000, 1, 1, &[], None, &thr, false);
        assert!(!empty.passed);
        assert!(empty.empty_or_near_empty);
        // clipped
        let clipped = vec![1.0f32; 24_000];
        let c = score_tts_pcm(&f, &clipped, 24_000, 1, 1, &[], None, &thr, false);
        assert!(!c.passed);
        assert!(c.clipped_samples > 0);
        // discontinuous join
        let mut pcm = vec![0.1f32; 12_000];
        pcm.extend(std::iter::repeat_n(-0.9f32, 12_000));
        let j = score_tts_pcm(&f, &pcm, 24_000, 1, 2, &[12_000], None, &thr, false);
        assert!(j.join_discontinuity > 0.5);
        // truncated flag
        let ok_pcm: Vec<f32> = (0..24_000).map(|i| 0.2 * (i as f32 * 0.01).sin()).collect();
        let t = score_tts_pcm(&f, &ok_pcm, 24_000, 1, 1, &[], Some(100), &thr, true);
        assert!(!t.passed);
        assert!(t.failures.iter().any(|x| x == "truncated"));
    }

    #[test]
    fn clean_pcm_passes() {
        let f = fix("t", "hello", "en-US", &["short"], None);
        let thr = TtsObjectiveThresholds::default();
        let pcm: Vec<f32> = (0..48_000)
            .map(|i| 0.25 * (i as f32 * 0.02).sin())
            .collect();
        let s = score_tts_pcm(&f, &pcm, 24_000, 1, 1, &[], Some(50), &thr, false);
        assert!(s.passed, "{:?}", s.failures);
        assert!(s.duration_ms >= 1000);
    }

    #[test]
    fn listening_aggregate_and_support_tier() {
        let mut by_model = BTreeMap::new();
        let ratings: Vec<_> = (0..20)
            .map(|i| ListeningRating {
                fixture_id: format!("f{i}"),
                blinded_label: "A".into(),
                intelligibility: 5,
                naturalness: 4,
                pronunciation: 4,
                join_smoothness: 4,
                critical_failure: false,
                notes: None,
            })
            .collect();
        by_model.insert("kitten-nano-int8".into(), ratings);
        let agg = aggregate_listening(&by_model);
        let a = agg.get("kitten-nano-int8").unwrap();
        assert_eq!(a.n_ratings, 20);
        assert!(a.median_intelligibility >= 4.0);
        let d = evaluate_support_tier("kitten-nano-int8", "Luna", true, Some(a), true);
        assert!(d.supported, "{:?}", d.reasons);

        let bad = evaluate_support_tier("x", "y", true, None, true);
        assert!(!bad.supported);
    }

    #[test]
    fn report_markdown_deterministic() {
        let pack = tts_production_pack();
        let f = &pack.fixtures[0];
        let thr = TtsObjectiveThresholds::default();
        let pcm: Vec<f32> = (0..24_000).map(|i| 0.1 * (i as f32 * 0.01).sin()).collect();
        let score = score_tts_pcm(f, &pcm, 24_000, 1, 1, &[], None, &thr, false);
        let r = TtsObjectiveReport::from_scores(
            &pack,
            "kitten-nano-int8",
            "Luna",
            vec![score],
            TtsRunIdentity::default(),
        );
        assert!(r.all_passed);
        let md = r.to_markdown();
        assert!(md.contains("TTS objective"));
        assert_eq!(r.to_json_pretty().unwrap(), r.to_json_pretty().unwrap());
    }
}
