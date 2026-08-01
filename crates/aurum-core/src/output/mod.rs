//! Output formatters: txt, srt, json — and secure file commit transactions.
//!
//! Formatters stream into any [`io::Write`] with a byte budget (JOE-1604).

pub mod transaction;

use crate::error::{Result, UserError};
#[cfg(test)]
use crate::providers::Segment;
use crate::providers::TranscriptionResult;
use std::io::{self, Write};
use std::path::Path;

pub use transaction::{commit_text, CommitMode, OutputTransaction, SymlinkPolicy};

/// Default maximum serialized output size (32 MiB).
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;

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

/// Writer that tracks bytes written and enforces a hard budget.
struct BudgetWriter<'a, W: Write> {
    inner: &'a mut W,
    written: usize,
    max: usize,
}

impl<'a, W: Write> BudgetWriter<'a, W> {
    fn new(inner: &'a mut W, max: usize) -> Self {
        Self {
            inner,
            written: 0,
            max: max.max(1),
        }
    }
}

impl<W: Write> Write for BudgetWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written.saturating_add(buf.len()) > self.max {
            return Err(io::Error::other(format!(
                "output exceeds maximum of {} bytes (would write {})",
                self.max,
                self.written.saturating_add(buf.len())
            )));
        }
        let n = self.inner.write(buf)?;
        self.written = self.written.saturating_add(n);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Format a transcription result into the requested representation.
///
/// Implemented on top of the streaming writer with the same limit policy.
pub fn format_result(result: &TranscriptionResult, format: OutputFormat) -> Result<String> {
    format_result_with_limit(result, format, DEFAULT_MAX_OUTPUT_BYTES)
}

/// Like [`format_result`] with an explicit byte budget.
pub fn format_result_with_limit(
    result: &TranscriptionResult,
    format: OutputFormat,
    max_bytes: usize,
) -> Result<String> {
    let mut buf = Vec::new();
    write_result_with_limit(result, format, &mut buf, max_bytes)?;
    // Strip trailing newline added by write for convenience parity with historical
    // format_result (txt had no trailing newline; srt/json vary). We keep the
    // streaming write's trailing newline only for write paths; format_result
    // trims a single trailing `\n` when present for txt/json compatibility.
    match format {
        OutputFormat::Txt => {
            if buf.last() == Some(&b'\n') {
                buf.pop();
            }
            String::from_utf8(buf).map_err(|e| {
                crate::error::TranscriptionError::internal(format!("utf-8 output: {e}"))
            })
        }
        OutputFormat::Srt | OutputFormat::Json => {
            // Keep content; ensure no extra double-newline from write helper.
            if buf.last() == Some(&b'\n') && format == OutputFormat::Json {
                // pretty json from serde doesn't need our trailing NL for string form
                // but tests compare parseable JSON — trailing NL is fine for parse.
            }
            String::from_utf8(buf).map_err(|e| {
                crate::error::TranscriptionError::internal(format!("utf-8 output: {e}"))
            })
        }
    }
}

/// Write formatted output to a writer (streaming; no intermediate full String for SRT cues).
pub fn write_result<W: Write>(
    result: &TranscriptionResult,
    format: OutputFormat,
    writer: W,
) -> Result<()> {
    write_result_with_limit(result, format, writer, DEFAULT_MAX_OUTPUT_BYTES)
}

/// Stream format into `writer` with a maximum output size.
pub fn write_result_with_limit<W: Write>(
    result: &TranscriptionResult,
    format: OutputFormat,
    mut writer: W,
    max_bytes: usize,
) -> Result<()> {
    let mut budget = BudgetWriter::new(&mut writer, max_bytes);
    match format {
        OutputFormat::Txt => write_txt(result, &mut budget),
        OutputFormat::Srt => write_srt(result, &mut budget),
        OutputFormat::Json => write_json(result, &mut budget),
    }
    .map_err(map_write_err)?;
    // Writers always terminate with a newline (txt/json) or cue newlines (srt).
    let _ = budget.written;
    Ok(())
}

fn map_write_err(e: io::Error) -> crate::error::TranscriptionError {
    let msg = e.to_string();
    if msg.contains("output exceeds maximum") {
        return UserError::Other {
            message: format!(
                "output too large: {msg}\n  Hint: raise the output byte budget or split the transcript."
            ),
        }
        .into();
    }
    // Broken pipe → user/environment, not internal panic.
    if e.kind() == io::ErrorKind::BrokenPipe {
        return crate::error::EnvironmentError::Other {
            message: "broken pipe while writing output".into(),
        }
        .into();
    }
    crate::error::EnvironmentError::Io(e).into()
}

/// Format and commit a transcription result through the shared output transaction.
pub fn write_result_to_path(
    result: &TranscriptionResult,
    format: OutputFormat,
    path: &Path,
    mode: CommitMode,
) -> Result<()> {
    write_result_to_path_with_limit(result, format, path, mode, DEFAULT_MAX_OUTPUT_BYTES)
}

/// Like [`write_result_to_path`] with an explicit byte budget.
pub fn write_result_to_path_with_limit(
    result: &TranscriptionResult,
    format: OutputFormat,
    path: &Path,
    mode: CommitMode,
    max_bytes: usize,
) -> Result<()> {
    OutputTransaction::new(path, mode).commit_with(|tmp| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp)
            .map_err(crate::error::EnvironmentError::Io)?;
        write_result_with_limit(result, format, &mut file, max_bytes)?;
        file.flush().map_err(crate::error::EnvironmentError::Io)?;
        file.sync_all()
            .map_err(crate::error::EnvironmentError::Io)?;
        Ok(())
    })
}

fn write_txt<W: Write>(result: &TranscriptionResult, w: &mut W) -> io::Result<()> {
    let text = result.text.trim();
    w.write_all(text.as_bytes())?;
    w.write_all(b"\n")?;
    Ok(())
}

fn write_srt<W: Write>(result: &TranscriptionResult, w: &mut W) -> io::Result<()> {
    if result.segments.is_empty() {
        if result.text.trim().is_empty() {
            return Ok(());
        }
        let end = if result.duration_secs > 0.0 {
            result.duration_secs
        } else {
            1.0
        };
        if !end.is_finite() || result.duration_secs < 0.0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid transcript duration for SRT",
            )); // InvalidData is intentional (not Other)
        }
        write!(
            w,
            "1\n{} --> {}\n{}\n",
            format_ts(0.0),
            format_ts(end),
            result.text.trim().replace(['\r', '\n'], " ")
        )?;
        return Ok(());
    }

    let mut cue = 1usize;
    for seg in &result.segments {
        if !seg.start().is_finite() || !seg.end().is_finite() || seg.end() < seg.start() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "invalid SRT timestamps at cue {cue}: start={} end={}",
                    seg.start(),
                    seg.end()
                ),
            ));
        }
        let text = seg.text().trim();
        if text.is_empty() {
            continue;
        }
        let text = text.replace(['\r', '\n'], " ");
        write!(
            w,
            "{}\n{} --> {}\n{}\n\n",
            cue,
            format_ts(seg.start()),
            format_ts(seg.end()),
            text
        )?;
        cue += 1;
    }
    Ok(())
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

fn write_json<W: Write>(result: &TranscriptionResult, w: &mut W) -> io::Result<()> {
    // Versioned external DTO (JOE-1614) — single contract for CLI/embed JSON.
    let payload = crate::dto::SttResultDto::from_result(result);
    serde_json::to_writer_pretty(&mut *w, &payload).map_err(io::Error::other)?;
    w.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> TranscriptionResult {
        TranscriptionResult::local(
            "Hello world. This is a test.".into(),
            vec![
                Segment::from_parts_unchecked(0.0, 1.2, "Hello world.".to_string()),
                Segment::from_parts_unchecked(1.4, 3.5, "This is a test.".to_string()),
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
        assert_eq!(v["schema_version"], 1);
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

    #[test]
    fn stream_matches_format_for_srt() {
        let r = sample_result();
        let mut stream = Vec::new();
        write_result(&r, OutputFormat::Srt, &mut stream).unwrap();
        let via_format = format_result(&r, OutputFormat::Srt).unwrap();
        // format_result may or may not strip; both should contain same cues.
        let stream_s = String::from_utf8(stream).unwrap();
        assert!(stream_s.contains("Hello world."));
        assert!(via_format.contains("Hello world."));
        assert_eq!(
            stream_s.replace("\r\n", "\n").trim(),
            via_format.replace("\r\n", "\n").trim()
        );
    }

    #[test]
    fn budget_rejects_large_output() {
        let r = sample_result();
        let err = format_result_with_limit(&r, OutputFormat::Json, 32).unwrap_err();
        assert!(err.to_string().contains("too large") || err.to_string().contains("maximum"));
    }

    #[test]
    fn invalid_srt_timestamps_rejected() {
        let mut r = sample_result();
        r.segments[0].set_end(f64::NAN);
        let err = write_result(&r, OutputFormat::Srt, Vec::new()).unwrap_err();
        assert!(err.to_string().contains("timestamp") || err.to_string().contains("invalid"));
    }

    #[test]
    fn large_segment_set_streams() {
        let segs: Vec<Segment> = (0..5000)
            .map(|i| Segment::from_parts_unchecked(i as f64, i as f64 + 0.5, format!("cue {i}")))
            .collect();
        let r = TranscriptionResult::local(
            "many".into(),
            segs,
            Some("en".into()),
            "base".into(),
            5000.0,
        );
        let mut out = Vec::new();
        write_result_with_limit(&r, OutputFormat::Srt, &mut out, 8 * 1024 * 1024).unwrap();
        assert!(out.len() > 10_000);
        assert!(out
            .windows(5)
            .any(|w| w == b"cue 0" || w.starts_with(b"cue ")));
    }
}
