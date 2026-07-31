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

/// Like [`normalize_result`] but returns an explicit repair report.
pub fn normalize_result_with_report(
    mut result: TranscriptionResult,
) -> (TranscriptionResult, NormalizationReport) {
    let mut report = NormalizationReport::default();

    // Backend / reliability consistency.
    if matches!(result.backend_kind, BackendKind::LlmAssisted) && result.timestamps_reliable {
        result.timestamps_reliable = false;
        report.events.push(NormalizationEvent {
            code: "backend_reliability".into(),
            detail: "LLM-assisted backend cannot claim reliable timestamps".into(),
        });
    }

    if !result.duration_secs.is_finite() || result.duration_secs < 0.0 {
        report.events.push(NormalizationEvent {
            code: "duration".into(),
            detail: format!("non-finite or negative duration {}", result.duration_secs),
        });
        result.duration_secs = 0.0;
    }

    let mut cleaned_segments = Vec::with_capacity(result.segments.len());
    for mut seg in result.segments.drain(..) {
        let before = seg.text.clone();
        seg.text = strip_markers(&seg.text);
        if seg.text != before {
            report.markers_stripped = true;
        }
        let trimmed = seg.text.trim();
        if trimmed.is_empty() || is_only_marker(trimmed) {
            report.dropped_segments += 1;
            report.events.push(NormalizationEvent {
                code: "drop_segment".into(),
                detail: "empty or marker-only segment".into(),
            });
            continue;
        }
        seg.text = trimmed.to_string();

        if !seg.start.is_finite() || !seg.end.is_finite() {
            report.dropped_segments += 1;
            report.events.push(NormalizationEvent {
                code: "drop_segment".into(),
                detail: "non-finite timestamps".into(),
            });
            continue;
        }

        let mut repaired = false;
        if result.duration_secs > 0.0 {
            let ns = seg.start.clamp(0.0, result.duration_secs);
            let ne = seg.end.clamp(0.0, result.duration_secs);
            if ns != seg.start || ne != seg.end {
                repaired = true;
            }
            seg.start = ns;
            seg.end = ne;
        } else {
            if seg.start < 0.0 {
                seg.start = 0.0;
                repaired = true;
            }
            if seg.end < 0.0 {
                seg.end = 0.0;
                repaired = true;
            }
        }
        if seg.end < seg.start {
            std::mem::swap(&mut seg.start, &mut seg.end);
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
    result.segments = cleaned_segments;

    if !result.segments.is_empty() {
        result.text = join_segment_text(&result.segments);
    } else {
        let before = result.text.clone();
        result.text = strip_markers(&result.text);
        if result.text != before {
            report.markers_stripped = true;
        }
        result.text = result.text.trim().to_string();
        if is_only_marker(&result.text) {
            result.text.clear();
        }
    }

    (result, report)
}

fn join_segment_text(segments: &[Segment]) -> String {
    let mut out = String::new();
    for seg in segments {
        let t = seg.text.trim();
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
    Some(Segment {
        start,
        end,
        text: text.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_nan_segments() {
        let r = TranscriptionResult::local(
            "x".into(),
            vec![
                Segment {
                    start: f64::NAN,
                    end: 1.0,
                    text: "bad".into(),
                },
                Segment {
                    start: 0.0,
                    end: 1.0,
                    text: "good".into(),
                },
            ],
            None,
            "m".into(),
            2.0,
        );
        let (out, report) = normalize_result_with_report(r);
        assert_eq!(out.segments.len(), 1);
        assert_eq!(out.segments[0].text, "good");
        assert!(report.dropped_segments >= 1);
    }

    #[test]
    fn llm_backend_clears_reliable_flag() {
        let mut r =
            TranscriptionResult::openrouter("hi".into(), vec![], None, "m".into(), 1.0, true);
        r.timestamps_reliable = true;
        let (out, report) = normalize_result_with_report(r);
        assert!(!out.timestamps_reliable);
        assert!(!report.is_clean());
    }

    #[test]
    fn validated_segment_rejects_inverted() {
        assert!(validated_segment(2.0, 1.0, "x").is_none());
        assert!(validated_segment(0.0, 1.0, "x").is_some());
    }
}
