//! Output formatters: txt, srt, json.

use crate::error::{Result, UserError};
use crate::providers::{Segment, TranscriptionResult};
use serde::Serialize;
use std::io::Write;

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
        let text = text.replace('\r', " ").replace('\n', " ");
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
    text: &'a str,
    language: Option<&'a str>,
    model: &'a str,
    provider: &'a str,
    duration_secs: f64,
    segments: &'a [Segment],
}

fn format_json(result: &TranscriptionResult) -> Result<String> {
    let payload = JsonOutput {
        text: &result.text,
        language: result.language.as_deref(),
        model: &result.model,
        provider: &result.provider,
        duration_secs: result.duration_secs,
        segments: &result.segments,
    };
    Ok(serde_json::to_string_pretty(&payload).map_err(|e| {
        crate::error::TranscriptionError::internal(format!("json serialize failed: {e}"))
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> TranscriptionResult {
        TranscriptionResult {
            text: "Hello world. This is a test.".into(),
            language: Some("en".into()),
            model: "base".into(),
            provider: "local".into(),
            duration_secs: 3.5,
            segments: vec![
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
        }
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
        assert!(v["segments"].as_array().unwrap().len() == 2);
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
