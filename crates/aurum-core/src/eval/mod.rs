//! Versioned quality evaluation helpers (JOE-1607).
//!
//! Offline, deterministic metrics for STT (WER/CER) and simple TTS objective
//! checks. Fixture corpora live under `evals/` at the repo root.

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
    EvalReport {
        corpus_version: corpus.version,
        corpus_name: corpus.name.clone(),
        model: model.into(),
        backend_kind: backend_kind.into(),
        stt_scores: scores,
        mean_wer,
        mean_cer,
    }
}

/// Built-in smoke corpus (no binary audio assets — text scoring only).
pub fn smoke_corpus() -> EvalCorpus {
    EvalCorpus {
        version: 1,
        name: "aurum-smoke-v1".into(),
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
                tags: vec!["numbers".into()],
                timestamps_expected_reliable: true,
                license: "synthetic CC0".into(),
            },
            SttFixture {
                id: "silence_empty".into(),
                language: "en".into(),
                reference: "".into(),
                audio: None,
                tags: vec!["silence".into()],
                timestamps_expected_reliable: true,
                license: "synthetic CC0".into(),
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
                tags: vec!["numbers".into()],
                duration_ms_min: Some(300),
                duration_ms_max: Some(8_000),
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
        assert_eq!(report.corpus_version, 1);
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("mean_wer"));
    }

    #[test]
    fn empty_ref_empty_hyp() {
        assert_eq!(word_error_rate("", ""), 0.0);
        assert_eq!(word_error_rate("", "hi"), 1.0);
    }
}
