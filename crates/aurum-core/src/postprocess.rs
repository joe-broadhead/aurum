//! Shared transcript cleanup applied after every provider (JOE-1609).
//!
//! Whisper (and some remote models) emit control / non-speech tokens that should
//! not leak into scripted `txt`/`srt`/`json` output. Normalization produces an
//! explicit [`NormalizationReport`] listing repairs and drops.

use crate::providers::{BackendKind, Segment, TranscriptionResult};
use serde::{Deserialize, Serialize};

/// Known non-speech / control markers frequently emitted by whisper.cpp.
const SPECIAL_MARKERS: &[&str] = &[
    "[BLANK_AUDIO]",
    "[blank_audio]",
    "[MUSIC]",
    "[Music]",
    "[music]",
    "[SILENCE]",
    "[Silence]",
    "[silence]",
    "[NOISE]",
    "[Noise]",
    "[noise]",
    "[INAUDIBLE]",
    "[Inaudible]",
    "[CLICK]",
    "[Click]",
    "[APPLAUSE]",
    "[Applause]",
    "[LAUGHTER]",
    "[Laughter]",
    "[COUGH]",
    "[Cough]",
    "♪",
    "♫",
];

/// One normalization repair or warning (deterministic, no provider payloads).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizationEvent {
    pub code: String,
    pub detail: String,
}

/// Report of repairs applied while normalizing a provider result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizationReport {
    pub events: Vec<NormalizationEvent>,
    pub dropped_segments: usize,
    pub repaired_timestamps: usize,
    pub markers_stripped: bool,
}

impl NormalizationReport {
    pub fn warnings(&self) -> Vec<String> {
        self.events
            .iter()
            .map(|e| format!("{}: {}", e.code, e.detail))
            .collect()
    }

    pub fn is_clean(&self) -> bool {
        self.events.is_empty()
    }
}

/// Clean provider output into a stable, scriptable result.
pub fn normalize_result(result: TranscriptionResult) -> TranscriptionResult {
    normalize_result_with_report(result).0
}

/// Conservative transcript degeneration detector (JOE-1650).
///
/// Flags unbounded n-gram loops (e.g. whisper hallucination of a short phrase
/// repeated dozens of times). Thresholds are intentionally high to avoid
/// flagging lyrics, stutters, or legitimate short repetitions.
///
/// Returns `Some(detail)` when the transcript looks degenerate.
pub fn detect_repetition_degeneration(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().filter(|w| !w.is_empty()).collect();
    if words.len() < 40 {
        return None;
    }
    // Count max run of identical 5-grams.
    const N: usize = 5;
    const MAX_SAME_NGRAM: usize = 12; // ~60 words of pure loop
    if words.len() < N + MAX_SAME_NGRAM {
        return None;
    }
    let mut best = 0usize;
    let mut best_phrase = String::new();
    let mut i = 0usize;
    while i + N <= words.len() {
        let phrase = &words[i..i + N];
        let mut count = 1usize;
        let mut j = i + N;
        while j + N <= words.len() && &words[j..j + N] == phrase {
            count += 1;
            j += N;
        }
        if count > best {
            best = count;
            best_phrase = phrase.join(" ");
        }
        if count >= MAX_SAME_NGRAM {
            return Some(format!(
                "repeated 5-gram {count}× (\"{best_phrase}\") — possible model degeneration"
            ));
        }
        i += 1;
    }
    // Also flag when a single 5-gram occupies >40% of the transcript via non-adjacent counts.
    use std::collections::HashMap;
    let mut freq: HashMap<String, usize> = HashMap::new();
    for w in words.windows(N) {
        *freq.entry(w.join(" ")).or_default() += 1;
    }
    if let Some((phrase, c)) = freq.into_iter().max_by_key(|(_, c)| *c) {
        if c >= MAX_SAME_NGRAM && (c * N * 100 / words.len()) >= 40 {
            return Some(format!(
                "5-gram occupies large fraction of transcript ({c}× \"{phrase}\") — possible model degeneration"
            ));
        }
    }
    let _ = best_phrase;
    let _ = best;
    None
}

/// Like [`normalize_result`] but returns an explicit repair report.
pub fn normalize_result_with_report(
    mut result: TranscriptionResult,
) -> (TranscriptionResult, NormalizationReport) {
    let mut report = NormalizationReport::default();

    if let Some(detail) = detect_repetition_degeneration(result.text()) {
        report.events.push(NormalizationEvent {
            code: "degeneration_repetition".into(),
            detail,
        });
    }

    // Backend / reliability consistency.
    if matches!(result.backend_kind(), BackendKind::LlmAssisted) && result.timestamps_reliable() {
        result.set_timestamps_reliable(false);
        report.events.push(NormalizationEvent {
            code: "backend_reliability".into(),
            detail: "LLM-assisted backend cannot claim reliable timestamps".into(),
        });
    }

    let duration = result.duration_secs();
    if !duration.is_finite() || duration < 0.0 {
        report.events.push(NormalizationEvent {
            code: "duration".into(),
            detail: format!("non-finite or negative duration {duration}"),
        });
        result.set_duration_secs(0.0);
    }
    let duration = result.duration_secs();

    let raw_segments = std::mem::take(result.segments_mut());
    let mut cleaned_segments = Vec::with_capacity(raw_segments.len());
    for mut seg in raw_segments {
        let before = seg.text().to_string();
        seg.set_text(strip_markers(seg.text()));
        if seg.text() != before {
            report.markers_stripped = true;
        }
        let trimmed = seg.text().trim();
        if trimmed.is_empty() || is_only_marker(trimmed) {
            report.dropped_segments += 1;
            report.events.push(NormalizationEvent {
                code: "drop_segment".into(),
                detail: "empty or marker-only segment".into(),
            });
            continue;
        }
        seg.set_text(trimmed.to_string());

        if !seg.start().is_finite() || !seg.end().is_finite() {
            report.dropped_segments += 1;
            report.events.push(NormalizationEvent {
                code: "drop_segment".into(),
                detail: "non-finite timestamps".into(),
            });
            continue;
        }

        let mut repaired = false;
        if duration > 0.0 {
            let ns = seg.start().clamp(0.0, duration);
            let ne = seg.end().clamp(0.0, duration);
            if ns != seg.start() || ne != seg.end() {
                repaired = true;
            }
            seg.set_start(ns);
            seg.set_end(ne);
        } else {
            if seg.start() < 0.0 {
                seg.set_start(0.0);
                repaired = true;
            }
            if seg.end() < 0.0 {
                seg.set_end(0.0);
                repaired = true;
            }
        }
        if seg.end() < seg.start() {
            let (a, b) = (seg.start(), seg.end());
            seg.set_start(b);
            seg.set_end(a);
            repaired = true;
            report.events.push(NormalizationEvent {
                code: "swap_timestamps".into(),
                detail: "inverted segment span swapped".into(),
            });
        }
        if repaired {
            report.repaired_timestamps += 1;
        }
        cleaned_segments.push(seg);
    }
    result.set_segments(cleaned_segments);

    if !result.segments().is_empty() {
        result.set_text(join_segment_text(result.segments()));
    } else {
        let before = result.text().to_string();
        let stripped = strip_markers(&before);
        if stripped != before {
            report.markers_stripped = true;
        }
        let trimmed = stripped.trim().to_string();
        if is_only_marker(&trimmed) {
            result.set_text(String::new());
        } else {
            result.set_text(trimmed);
        }
    }

    (result, report)
}

fn join_segment_text(segments: &[Segment]) -> String {
    let mut out = String::new();
    for seg in segments {
        let t = seg.text().trim();
        if t.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t);
    }
    out
}

fn strip_markers(text: &str) -> String {
    let mut out = text.to_string();
    for marker in SPECIAL_MARKERS {
        out = out.replace(marker, " ");
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_only_marker(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    SPECIAL_MARKERS
        .iter()
        .any(|m| t.eq_ignore_ascii_case(m.trim_matches(|c| c == '[' || c == ']')))
        || SPECIAL_MARKERS.iter().any(|m| t.eq_ignore_ascii_case(m))
}

/// UTF-8–safe truncation for error messages (never panics on char boundaries).
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
}

/// Validated segment constructor (rejects NaN/inverted spans).
pub fn validated_segment(start: f64, end: f64, text: impl Into<String>) -> Option<Segment> {
    if !start.is_finite() || !end.is_finite() || end < start {
        return None;
    }
    let text = text.into();
    if text.trim().is_empty() {
        return None;
    }
    Some(Segment::from_parts_unchecked(
        start,
        end,
        text.trim().to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_extreme_ngram_loop() {
        let phrase = "feature and i think it's ";
        let text = phrase.repeat(40);
        let detail = detect_repetition_degeneration(&text).expect("should flag loop");
        assert!(detail.contains("repeated") || detail.contains("fraction"));
    }

    #[test]
    fn ordinary_text_not_flagged() {
        let text = "There's something that a lot of people do when they're working with AI \
                    to write code that totally drives me crazy and it's sort of something \
                    that I've been railing against for a while now. The thing that I've \
                    noticed is people tend to think I need to create a spec for AI.";
        assert!(detect_repetition_degeneration(text).is_none());
    }

    #[test]
    fn short_legitimate_repetition_ok() {
        let text = "no no no I said yes yes yes please please please thank you thank you";
        assert!(detect_repetition_degeneration(text).is_none());
    }

    #[test]
    fn drops_nan_segments() {
        let r = TranscriptionResult::local(
            "x".into(),
            vec![
                Segment::from_parts_unchecked(f64::NAN, 1.0, "bad".to_string()),
                Segment::from_parts_unchecked(0.0, 1.0, "good".to_string()),
            ],
            None,
            "m".into(),
            2.0,
        );
        let (out, report) = normalize_result_with_report(r);
        assert_eq!(out.segments().len(), 1);
        assert_eq!(out.segments()[0].text(), "good");
        assert!(report.dropped_segments >= 1);
    }

    #[test]
    fn llm_backend_clears_reliable_flag() {
        let mut r =
            TranscriptionResult::openrouter("hi".into(), vec![], None, "m".into(), 1.0, true);
        r.set_timestamps_reliable(true);
        let (out, report) = normalize_result_with_report(r);
        assert!(!out.timestamps_reliable());
        assert!(!report.is_clean());
    }

    #[test]
    fn validated_segment_rejects_inverted() {
        assert!(validated_segment(2.0, 1.0, "x").is_none());
        assert!(validated_segment(0.0, 1.0, "x").is_some());
    }
}
