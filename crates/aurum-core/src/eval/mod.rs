//! Versioned quality evaluation helpers (JOE-1607 / JOE-2216).
//!
//! Offline, deterministic metrics for STT (WER/CER) and simple TTS objective
//! checks. Fixture corpora live under `evals/` at the repo root.
//!
//! The production STT quality observatory (schemas, budgets, scorecards) lives
//! in [`observatory`].

pub mod observatory;
pub mod perf;
pub mod tts_listening;

pub use observatory::{
    allowed_mean_wer, budget_exit_code, compare_stt_budget, observatory_core_budget_tiny,
    observatory_core_corpus, score_observatory_fixture, AssetResolution, BudgetComparison,
    BudgetFinding, BudgetSeverity, CorpusCoverage, NORMALIZATION_POLICY_VERSION,
    OBSERVATORY_SCHEMA_VERSION, ObservatoryCorpus, ObservatoryFixture, ObservatoryFixtureScore,
    ObservatoryReport, ObservatoryScoreExtras, RunIdentity, STT_OBSERVATORY_EVIDENCE_VERSION,
    SttBudget,
};
pub use perf::{
    compare_perf_budget, percentile_sorted, perf_budget_exit_code, perf_scenario_catalogue,
    tier_a_profile_templates, HardwareTier, NamedHardwareProfile, PERF_EVIDENCE_VERSION,
    PERF_SCHEMA_VERSION, PerfBudget, PerfComparison, PerfFinding, PerfReport, PerfScenario,
    PerfScenarioBudget, PerfScenarioResult, PerfSeverity,
};
pub use tts_listening::{
    aggregate_listening, evaluate_support_tier, join_discontinuity_score, score_tts_pcm,
    tts_local_matrix, tts_production_pack, ListeningAggregate, ListeningRating, ListeningReport,
    SupportTierDecision, TtsEvalFixture, TtsEvalPack, TtsEvalParticipation, TtsObjectiveReport,
    TtsObjectiveScore, TtsObjectiveThresholds, TtsRunIdentity, TTS_EVAL_SCHEMA_VERSION,
    TTS_EVIDENCE_VERSION,
};

use serde::{Deserialize, Serialize};

/// One STT reference fixture (text-only; audio path is optional for offline text scoring).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SttFixture {
    pub id: String,
    pub language: String,
    /// Normalized reference transcript.
    pub reference: String,
    /// Relative path to audio under the corpus root (optional for pure text tests).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// Tags: clean, noisy, short, long, silence, …
    #[serde(default)]
    pub tags: Vec<String>,
    /// When true, timestamps are expected to be reliable (ASR, not LLM-assisted).
    #[serde(default = "default_true")]
    pub timestamps_expected_reliable: bool,
    /// Provenance / licensing note.
    #[serde(default)]
    pub license: String,
}

fn default_true() -> bool {
    true
}

/// TTS text fixture for pronunciation / chunk-join checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsFixture {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Expected approximate duration bounds (ms), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms_min: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms_max: Option<u64>,
    #[serde(default)]
    pub license: String,
}

/// Corpus manifest (versioned).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalCorpus {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub stt: Vec<SttFixture>,
    #[serde(default)]
    pub tts: Vec<TtsFixture>,
}

/// STT scoring result for one hypothesis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SttScore {
    pub fixture_id: String,
    pub wer: f64,
    pub cer: f64,
    pub empty_hypothesis: bool,
    pub ref_words: usize,
    pub hyp_words: usize,
    /// True when backend timestamps are claimed reliable (host-supplied).
    pub timestamps_reliable: bool,
    /// Non-empty hypothesis when the reference is empty (silence false positive).
    #[serde(default)]
    pub silence_false_positive: bool,
    /// Rough degeneration proxy: max run of identical tokens / hyp length.
    #[serde(default)]
    pub repetition_ratio: f64,
}

/// Aggregate report (machine-readable).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalReport {
    pub corpus_version: u32,
    pub corpus_name: String,
    pub model: String,
    pub backend_kind: String,
    pub stt_scores: Vec<SttScore>,
    pub mean_wer: f64,
    pub mean_cer: f64,
    /// Count of silence false positives in this report.
    #[serde(default)]
    pub silence_false_positives: u32,
    /// Mean repetition ratio across non-empty hypotheses.
    #[serde(default)]
    pub mean_repetition_ratio: f64,
    /// Optional hardware / run metadata for release retention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Normalize text for WER: lowercase, strip punctuation, collapse whitespace.
pub fn normalize_transcript(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            for c in ch.to_lowercase() {
                out.push(c);
            }
            last_space = false;
        } else if ch.is_whitespace() && !last_space {
            out.push(' ');
            last_space = true;
        }
        // drop punctuation
    }
    out.trim().to_string()
}

fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().filter(|w| !w.is_empty()).collect()
}

/// Word error rate via classic Levenshtein on word tokens.
pub fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r = normalize_transcript(reference);
    let h = normalize_transcript(hypothesis);
    let rw = words(&r);
    let hw = words(&h);
    if rw.is_empty() {
        return if hw.is_empty() { 0.0 } else { 1.0 };
    }
    let dist = levenshtein(&rw, &hw);
    dist as f64 / rw.len() as f64
}

/// Character error rate on normalized strings (spaces kept as characters).
pub fn char_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r: Vec<char> = normalize_transcript(reference).chars().collect();
    let h: Vec<char> = normalize_transcript(hypothesis).chars().collect();
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let dist = levenshtein(&r, &h);
    dist as f64 / r.len() as f64
}

fn levenshtein<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1) // deletion
                .min(curr[j - 1] + 1) // insertion
                .min(prev[j - 1] + cost); // substitution
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// Silence false positive: non-empty hypothesis for an empty reference.
pub fn silence_false_positive(reference: &str, hypothesis: &str) -> bool {
    normalize_transcript(reference).is_empty() && !normalize_transcript(hypothesis).is_empty()
}

/// Degeneration proxy: longest run of identical tokens over hypothesis length.
pub fn repetition_ratio(hypothesis: &str) -> f64 {
    let normalized = normalize_transcript(hypothesis);
    let hw = words(&normalized);
    if hw.is_empty() {
        return 0.0;
    }
    let mut best = 1usize;
    let mut run = 1usize;
    for w in hw.windows(2) {
        if w[0] == w[1] {
            run += 1;
            best = best.max(run);
        } else {
            run = 1;
        }
    }
    best as f64 / hw.len() as f64
}

/// Score one STT hypothesis against a fixture.
pub fn score_stt(fixture: &SttFixture, hypothesis: &str, timestamps_reliable: bool) -> SttScore {
    let wer = word_error_rate(&fixture.reference, hypothesis);
    let cer = char_error_rate(&fixture.reference, hypothesis);
    let hyp_n = words(&normalize_transcript(hypothesis)).len();
    let ref_n = words(&normalize_transcript(&fixture.reference)).len();
    SttScore {
        fixture_id: fixture.id.clone(),
        wer,
        cer,
        empty_hypothesis: hypothesis.trim().is_empty(),
        ref_words: ref_n,
        hyp_words: hyp_n,
        timestamps_reliable,
        silence_false_positive: silence_false_positive(&fixture.reference, hypothesis),
        repetition_ratio: repetition_ratio(hypothesis),
    }
}

/// Build an aggregate report from scores.
pub fn build_report(
    corpus: &EvalCorpus,
    model: &str,
    backend_kind: &str,
    scores: Vec<SttScore>,
) -> EvalReport {
    let n = scores.len().max(1) as f64;
    let mean_wer = scores.iter().map(|s| s.wer).sum::<f64>() / n;
    let mean_cer = scores.iter().map(|s| s.cer).sum::<f64>() / n;
    let silence_false_positives = scores.iter().filter(|s| s.silence_false_positive).count() as u32;
    let rep_scores: Vec<f64> = scores
        .iter()
        .filter(|s| s.hyp_words > 0)
        .map(|s| s.repetition_ratio)
        .collect();
    let mean_repetition_ratio = if rep_scores.is_empty() {
        0.0
    } else {
        rep_scores.iter().sum::<f64>() / rep_scores.len() as f64
    };
    EvalReport {
        corpus_version: corpus.version,
        corpus_name: corpus.name.clone(),
        model: model.into(),
        backend_kind: backend_kind.into(),
        stt_scores: scores,
        mean_wer,
        mean_cer,
        silence_false_positives,
        mean_repetition_ratio,
        hardware_profile: None,
        notes: None,
    }
}

/// Built-in smoke corpus (synthetic text + optional synthetic audio paths).
///
/// Audio under `evals/audio/` is generated CC0 PCM (silence / tone), not speech.
/// Real multi-accent speech corpora remain external/private with the same schema.
pub fn smoke_corpus() -> EvalCorpus {
    EvalCorpus {
        version: 2,
        name: "aurum-smoke-v2".into(),
        stt: vec![
            SttFixture {
                id: "clean_short_en".into(),
                language: "en".into(),
                reference: "hello world".into(),
                audio: None,
                tags: vec!["clean".into(), "short".into()],
                timestamps_expected_reliable: true,
                license: "synthetic CC0".into(),
            },
            SttFixture {
                id: "numbers_en".into(),
                language: "en".into(),
                reference: "the meeting is at 3 30 pm".into(),
                audio: None,
                tags: vec!["numbers".into(), "punctuation".into()],
                timestamps_expected_reliable: true,
                license: "synthetic CC0".into(),
            },
            SttFixture {
                id: "silence_empty".into(),
                language: "en".into(),
                reference: "".into(),
                audio: Some("audio/silence_1s.wav".into()),
                tags: vec!["silence".into()],
                timestamps_expected_reliable: true,
                license: "synthetic CC0".into(),
            },
            SttFixture {
                id: "tone_non_speech".into(),
                language: "en".into(),
                reference: "".into(),
                audio: Some("audio/tone_440_1s.wav".into()),
                tags: vec!["noise".into(), "music".into(), "non_speech".into()],
                timestamps_expected_reliable: true,
                license: "synthetic CC0".into(),
            },
            SttFixture {
                id: "long_phrase_en".into(),
                language: "en".into(),
                reference: "the quick brown fox jumps over the lazy dog near the river bank"
                    .into(),
                audio: None,
                tags: vec!["clean".into(), "long".into()],
                timestamps_expected_reliable: true,
                license: "synthetic CC0".into(),
            },
            SttFixture {
                id: "accent_placeholder_en".into(),
                language: "en".into(),
                reference: "schedule the call for tomorrow morning".into(),
                audio: None,
                tags: vec!["accent".into(), "placeholder".into()],
                timestamps_expected_reliable: true,
                license: "synthetic CC0 — replace with licensed multi-accent speech".into(),
            },
        ],
        tts: vec![
            TtsFixture {
                id: "tts_short".into(),
                text: "Hello from Aurum.".into(),
                tags: vec!["short".into()],
                duration_ms_min: Some(200),
                duration_ms_max: Some(5_000),
                license: "synthetic CC0".into(),
            },
            TtsFixture {
                id: "tts_numbers".into(),
                text: "Call me at extension 42.".into(),
                tags: vec!["numbers".into(), "abbreviations".into()],
                duration_ms_min: Some(300),
                duration_ms_max: Some(8_000),
                license: "synthetic CC0".into(),
            },
            TtsFixture {
                id: "tts_long_join".into(),
                text: "First sentence ends here. Second sentence starts now and continues for a bit longer.".into(),
                tags: vec!["long".into(), "join".into()],
                duration_ms_min: Some(500),
                duration_ms_max: Some(20_000),
                license: "synthetic CC0".into(),
            },
        ],
    }
}

/// Objective TTS duration check (not MOS).
pub fn tts_duration_in_range(fixture: &TtsFixture, duration_ms: u64) -> bool {
    if let Some(min) = fixture.duration_ms_min {
        if duration_ms < min {
            return false;
        }
    }
    if let Some(max) = fixture.duration_ms_max {
        if duration_ms > max {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_match_zero_wer() {
        assert_eq!(word_error_rate("Hello, world!", "hello world"), 0.0);
    }

    #[test]
    fn one_sub_wer() {
        let wer = word_error_rate("a b c", "a x c");
        assert!((wer - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn cer_basic() {
        let cer = char_error_rate("abc", "axc");
        assert!((cer - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn smoke_corpus_scores() {
        let c = smoke_corpus();
        let scores: Vec<_> = c
            .stt
            .iter()
            .map(|f| score_stt(f, &f.reference, true))
            .collect();
        let report = build_report(&c, "tiny-q5_1", "asr", scores);
        assert_eq!(report.mean_wer, 0.0);
        assert_eq!(report.corpus_version, 2);
        assert_eq!(report.silence_false_positives, 0);
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("mean_wer"));
        assert!(json.contains("silence_false_positives"));
    }

    #[test]
    fn silence_fp_and_repetition() {
        assert!(silence_false_positive("", "hello hello hello"));
        assert!(!silence_false_positive("", ""));
        let r = repetition_ratio("yes yes yes yes no");
        assert!(r >= 0.5);
    }

    #[test]
    fn empty_ref_empty_hyp() {
        assert_eq!(word_error_rate("", ""), 0.0);
        assert_eq!(word_error_rate("", "hi"), 1.0);
    }
}
