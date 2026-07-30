//! Output formatters: txt, srt, json — and secure file commit transactions.

pub mod transaction;

use crate::error::{Result, UserError};
use crate::providers::{Segment, TranscriptionResult};
use serde::Serialize;
use std::io::Write;
use std::path::Path;

pub use transaction::{commit_text, CommitMode, OutputTransaction, SymlinkPolicy};

/// Supported output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Txt,
    Srt,
    Json,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "txt" | "text" => Ok(Self::Txt),
            "srt" => Ok(Self::Srt),
            "json" => Ok(Self::Json),
            other => Err(UserError::InvalidOutputFormat {
                format: other.to_string(),
            }
            .into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Srt => "srt",
            Self::Json => "json",
        }
    }

    pub fn default_extension(self) -> &'static str {
        self.as_str()
    }
}

/// Format a transcription result into the requested representation.
pub fn format_result(result: &TranscriptionResult, format: OutputFormat) -> Result<String> {
    match format {
        OutputFormat::Txt => Ok(format_txt(result)),
        OutputFormat::Srt => Ok(format_srt(result)),
        OutputFormat::Json => format_json(result),
    }
}

/// Write formatted output to a writer.
pub fn write_result<W: Write>(
    result: &TranscriptionResult,
    format: OutputFormat,
    mut writer: W,
) -> Result<()> {
    let body = format_result(result, format)?;
    writer.write_all(body.as_bytes())?;
    if !body.ends_with('\n') {
        writer.write_all(b"\n")?;
    }
    Ok(())
}

/// Format and commit a transcription result through the shared output transaction.
pub fn write_result_to_path(
    result: &TranscriptionResult,
    format: OutputFormat,
    path: &Path,
    mode: CommitMode,
) -> Result<()> {
    let body = format_result(result, format)?;
    commit_text(path, &body, mode)
}

fn format_txt(result: &TranscriptionResult) -> String {
    result.text.trim().to_string()
}

fn format_srt(result: &TranscriptionResult) -> String {
    if result.segments.is_empty() {
        // Single cue covering the whole transcript when no segments exist.
        if result.text.trim().is_empty() {
            return String::new();
        }
        let end = if result.duration_secs > 0.0 {
            result.duration_secs
        } else {
            1.0
        };
        return format!(
            "1\n{} --> {}\n{}\n",
            format_ts(0.0),
            format_ts(end),
            result.text.trim()
        );
    }

    let mut out = String::new();
    let mut cue = 1usize;
    for seg in &result.segments {
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }
        // SRT forbids unescaped newlines inside cue text — flatten.
        let text = text.replace(['\r', '\n'], " ");
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            cue,
            format_ts(seg.start),
            format_ts(seg.end),
            text
        ));
        cue += 1;
    }
    out
}

/// SRT timestamp: HH:MM:SS,mmm
fn format_ts(secs: f64) -> String {
    let total_ms = (secs.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_secs = total_ms / 1000;
    let s = total_secs % 60;
    let total_mins = total_secs / 60;
    let m = total_mins % 60;
    let h = total_mins / 60;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

#[derive(Serialize)]
struct JsonOutput<'a> {
    /// Rendered transcript text (post-cleanup when applied).
    text: &'a str,
    language: Option<&'a str>,
    model: &'a str,
    provider: &'a str,
    duration_secs: f64,
    backend_kind: crate::providers::BackendKind,
    /// False for LLM-assisted backends — segment times are best-effort only.
    timestamps_reliable: bool,
    cleanup_style: crate::cleanup::CleanupStyle,
    #[serde(skip_serializing_if = "Option::is_none")]
    cleanup_provider: Option<crate::cleanup::CleanupProviderKind>,
    /// Pre-cleanup ASR text when cleanup rewrote content (raw representation).
    #[serde(skip_serializing_if = "Option::is_none")]
    original_text: Option<&'a str>,
    /// Pre-cleanup ASR segments when cleanup rewrote or cleared timings.
    #[serde(skip_serializing_if = "Option::is_none")]
    original_segments: Option<&'a [Segment]>,
    /// Segment policy applied during cleanup (`keep` / `clear` / `per-segment`).
    #[serde(skip_serializing_if = "Option::is_none")]
    cleanup_segment_policy: Option<&'a str>,
    segments: &'a [Segment],
}

fn format_json(result: &TranscriptionResult) -> Result<String> {
    let payload = JsonOutput {
        text: &result.text,
        language: result.language.as_deref(),
        model: &result.model,
        provider: &result.provider,
        duration_secs: result.duration_secs,
        backend_kind: result.backend_kind,
        timestamps_reliable: result.timestamps_reliable,
        cleanup_style: result.cleanup_style,
        cleanup_provider: result.cleanup_provider,
        original_text: result.original_text.as_deref(),
        original_segments: result.original_segments.as_deref(),
        cleanup_segment_policy: result.cleanup_segment_policy.map(|p| p.as_str()),
        segments: &result.segments,
    };
    serde_json::to_string_pretty(&payload).map_err(|e| {
        crate::error::TranscriptionError::internal(format!("json serialize failed: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> TranscriptionResult {
        TranscriptionResult::local(
            "Hello world. This is a test.".into(),
            vec![
                Segment {
                    start: 0.0,
                    end: 1.2,
                    text: "Hello world.".into(),
                },
                Segment {
                    start: 1.4,
                    end: 3.5,
                    text: "This is a test.".into(),
                },
            ],
            Some("en".into()),
            "base".into(),
            3.5,
        )
    }

    #[test]
    fn txt_is_plain() {
        let s = format_result(&sample_result(), OutputFormat::Txt).unwrap();
        assert_eq!(s, "Hello world. This is a test.");
    }

    #[test]
    fn srt_has_cues() {
        let s = format_result(&sample_result(), OutputFormat::Srt).unwrap();
        assert!(s.contains("1\n"));
        assert!(s.contains("00:00:00,000 --> 00:00:01,200"));
        assert!(s.contains("Hello world."));
        assert!(s.contains("2\n"));
        assert!(s.contains("This is a test."));
    }

    #[test]
    fn json_roundtrip_fields() {
        let s = format_result(&sample_result(), OutputFormat::Json).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["text"], "Hello world. This is a test.");
        assert_eq!(v["model"], "base");
        assert_eq!(v["provider"], "local");
        assert_eq!(v["language"], "en");
        assert_eq!(v["timestamps_reliable"], true);
        assert_eq!(v["backend_kind"], "asr");
        assert_eq!(v["cleanup_style"], "raw");
        assert!(v.get("cleanup_provider").is_none() || v["cleanup_provider"].is_null());
        assert!(v.get("original_text").is_none() || v["original_text"].is_null());
        assert!(v["segments"].as_array().unwrap().len() == 2);
    }

    #[test]
    fn json_includes_cleanup_metadata() {
        let mut r = sample_result();
        r.cleanup_style = crate::cleanup::CleanupStyle::Clean;
        r.cleanup_provider = Some(crate::cleanup::CleanupProviderKind::Rules);
        r.original_text = Some("um hello".into());
        r.text = "Hello.".into();
        let s = format_result(&r, OutputFormat::Json).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["cleanup_style"], "clean");
        assert_eq!(v["cleanup_provider"], "rules");
        assert_eq!(v["original_text"], "um hello");
        assert_eq!(v["text"], "Hello.");
    }

    #[test]
    fn parse_formats() {
        assert_eq!(OutputFormat::parse("txt").unwrap(), OutputFormat::Txt);
        assert_eq!(OutputFormat::parse("SRT").unwrap(), OutputFormat::Srt);
        assert_eq!(OutputFormat::parse("json").unwrap(), OutputFormat::Json);
        assert!(OutputFormat::parse("docx").is_err());
    }

    #[test]
    fn format_ts_values() {
        assert_eq!(format_ts(0.0), "00:00:00,000");
        assert_eq!(format_ts(61.5), "00:01:01,500");
        assert_eq!(format_ts(3661.001), "01:01:01,001");
    }
}
