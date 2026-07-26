//! Text and path validation helpers for TTS.

use crate::error::{Result, UserError};
use std::path::Path;

/// Default max input characters before truncation / rejection.
pub const DEFAULT_MAX_CHARS: usize = 5_000;
/// Default wall-clock synthesis timeout.
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;
pub const SPEAKING_RATE_MIN: f32 = 0.5;
pub const SPEAKING_RATE_MAX: f32 = 2.0;

/// Validated / truncated text ready for synthesis.
#[derive(Debug, Clone)]
pub struct PreparedText {
    pub text: String,
    pub text_chars: usize,
    pub text_truncated: bool,
}

/// Reject empty / whitespace-only text (user error).
pub fn validate_text(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(UserError::Other {
            message: "TTS text is empty (provide positional text, '-', or --input-file)".into(),
        }
        .into());
    }
    Ok(())
}

/// Trim, enforce max chars, and report truncation.
///
/// Truncation prefers a word boundary near the limit when possible.
pub fn prepare_text(text: &str, max_chars: usize) -> Result<PreparedText> {
    validate_text(text)?;
    let trimmed = text.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= max_chars {
        return Ok(PreparedText {
            text: trimmed.to_string(),
            text_chars: chars.len(),
            text_truncated: false,
        });
    }
    // Truncate at last whitespace before limit when possible.
    let mut end = max_chars;
    if let Some(pos) = chars[..max_chars].iter().rposition(|c| c.is_whitespace()) {
        if pos > max_chars / 2 {
            end = pos;
        }
    }
    let truncated: String = chars[..end].iter().collect();
    Ok(PreparedText {
        text: truncated.trim_end().to_string(),
        text_chars: end,
        text_truncated: true,
    })
}

/// Clamp speaking rate into the supported range.
pub fn clamp_speaking_rate(rate: f32) -> f32 {
    if !rate.is_finite() {
        return 1.0;
    }
    rate.clamp(SPEAKING_RATE_MIN, SPEAKING_RATE_MAX)
}

/// Basic path safety for output files (refuse empty / bare directory).
pub fn validate_output_path(path: &Path) -> Result<()> {
    let s = path.as_os_str();
    if s.is_empty() {
        return Err(UserError::Other {
            message: "output path is empty".into(),
        }
        .into());
    }
    if path.file_name().map(|n| n.is_empty()).unwrap_or(true) {
        return Err(UserError::Other {
            message: format!(
                "output path '{}' must be a file path, not a directory",
                path.display()
            ),
        }
        .into());
    }
    Ok(())
}

/// Check overwrite policy: refuse existing non-empty file without force.
pub fn check_overwrite(path: &Path, force: bool) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let meta = std::fs::metadata(path)?;
    if meta.len() == 0 {
        return Ok(());
    }
    if force {
        return Ok(());
    }
    Err(UserError::Other {
        message: format!(
            "output file already exists: {}\n  Hint: pass --force to overwrite, or choose another path.",
            path.display()
        ),
    }
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn empty_text_rejected() {
        assert!(validate_text("").is_err());
        assert!(validate_text("   \n\t").is_err());
    }

    #[test]
    fn prepare_truncates() {
        let long = "word ".repeat(20);
        let p = prepare_text(&long, 30).unwrap();
        assert!(p.text_truncated);
        assert!(p.text.chars().count() <= 30);
        assert!(!p.text.is_empty());
    }

    #[test]
    fn prepare_no_trunc_under_limit() {
        let p = prepare_text("hello", 100).unwrap();
        assert!(!p.text_truncated);
        assert_eq!(p.text, "hello");
        assert_eq!(p.text_chars, 5);
    }

    #[test]
    fn rate_clamp() {
        assert_eq!(clamp_speaking_rate(1.0), 1.0);
        assert_eq!(clamp_speaking_rate(0.1), SPEAKING_RATE_MIN);
        assert_eq!(clamp_speaking_rate(9.0), SPEAKING_RATE_MAX);
        assert_eq!(clamp_speaking_rate(f32::NAN), 1.0);
    }

    #[test]
    fn overwrite_policy() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.wav");
        std::fs::write(&path, b"not empty").unwrap();
        assert!(check_overwrite(&path, false).is_err());
        assert!(check_overwrite(&path, true).is_ok());
        let empty = dir.path().join("empty.wav");
        std::fs::write(&empty, b"").unwrap();
        assert!(check_overwrite(&empty, false).is_ok());
    }
}
