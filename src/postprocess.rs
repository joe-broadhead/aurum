//! Shared transcript cleanup applied after every provider.
//!
//! Whisper (and some remote models) emit control / non-speech tokens that should
//! not leak into scripted `txt`/`srt`/`json` output.

use crate::providers::{Segment, TranscriptionResult};

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

/// Clean provider output into a stable, scriptable result.
pub fn normalize_result(mut result: TranscriptionResult) -> TranscriptionResult {
    // Drop / strip special markers from segments.
    let mut cleaned_segments = Vec::with_capacity(result.segments.len());
    for mut seg in result.segments.drain(..) {
        seg.text = strip_markers(&seg.text);
        let trimmed = seg.text.trim();
        if trimmed.is_empty() || is_only_marker(trimmed) {
            continue;
        }
        seg.text = trimmed.to_string();
        // Clamp timestamps into [0, duration].
        if result.duration_secs > 0.0 {
            seg.start = seg.start.clamp(0.0, result.duration_secs);
            seg.end = seg.end.clamp(seg.start, result.duration_secs);
        } else {
            seg.start = seg.start.max(0.0);
            seg.end = seg.end.max(seg.start);
        }
        cleaned_segments.push(seg);
    }
    result.segments = cleaned_segments;

    // Rebuild full text from segments when available (keeps txt/srt/json aligned).
    if !result.segments.is_empty() {
        result.text = join_segment_text(&result.segments);
    } else {
        result.text = strip_markers(&result.text);
        result.text = result.text.trim().to_string();
        if is_only_marker(&result.text) {
            result.text.clear();
        }
    }

    result
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
    // Collapse whitespace.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_only_marker(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    SPECIAL_MARKERS.iter().any(|m| t.eq_ignore_ascii_case(m))
        || (t.starts_with('[') && t.ends_with(']') && t.len() <= 32)
}

/// UTF-8–safe truncation for error messages (never panics on char boundaries).
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_blank_audio() {
        let r = normalize_result(TranscriptionResult {
            text: "[BLANK_AUDIO]".into(),
            segments: vec![Segment {
                start: 0.0,
                end: 10.0,
                text: "[BLANK_AUDIO]".into(),
            }],
            language: Some("en".into()),
            model: "tiny".into(),
            provider: "local".into(),
            duration_secs: 3.0,
        });
        assert!(r.text.is_empty(), "got {:?}", r.text);
        assert!(r.segments.is_empty());
    }

    #[test]
    fn clamps_timestamps() {
        let r = normalize_result(TranscriptionResult {
            text: "Hello".into(),
            segments: vec![Segment {
                start: -1.0,
                end: 99.0,
                text: "Hello".into(),
            }],
            language: None,
            model: "tiny".into(),
            provider: "local".into(),
            duration_secs: 5.0,
        });
        assert_eq!(r.segments[0].start, 0.0);
        assert_eq!(r.segments[0].end, 5.0);
    }

    #[test]
    fn truncate_multibyte_safe() {
        let s = "hello 你好世界";
        let t = truncate_chars(s, 8);
        assert!(t.ends_with('…'));
        assert!(t.is_char_boundary(t.len() - '…'.len_utf8()) || t.ends_with('…'));
        // Must not panic and must be valid UTF-8 (guaranteed by String).
        let _ = t.len();
    }
}
