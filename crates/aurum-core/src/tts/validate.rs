//! Text and path validation helpers for TTS.

use crate::error::{Result, UserError};
use std::path::Path;

/// Default max input characters before truncation / rejection.
pub const DEFAULT_MAX_CHARS: usize = 5_000;
/// Default wall-clock synthesis timeout.
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;
pub const SPEAKING_RATE_MIN: f32 = 0.5;
pub const SPEAKING_RATE_MAX: f32 = 2.0;

/// UTF-8 worst-case bytes per Unicode scalar + small framing overhead for reads.
const READ_BYTES_PER_CHAR: usize = 4;
const READ_BYTE_OVERHEAD: usize = 4096;

/// Validated text ready for synthesis.
///
/// Policy is **complete-or-error**: text is never silently truncated. Character
/// budget is a safety bound (OOM / abuse); model phoneme capacity is enforced
/// separately during chunking.
#[derive(Debug, Clone)]
pub struct PreparedText {
    pub text: String,
    pub text_chars: usize,
    /// Always `false` under the complete-or-error policy.
    pub text_truncated: bool,
}

/// Byte budget for loading TTS text from a file or stdin before full allocation.
///
/// Tied to `max_chars` so a huge input fails as a user error instead of OOM.
pub fn tts_input_byte_budget(max_chars: usize) -> usize {
    max_chars
        .saturating_mul(READ_BYTES_PER_CHAR)
        .saturating_add(READ_BYTE_OVERHEAD)
}

/// Normalize / accept only English locales the G2P engine actually supports.
///
/// Empty input defaults to `en`. Unsupported tags are a user error (honest
/// failure rather than echoing a language the engine did not use).
pub fn normalize_tts_language(lang: &str) -> Result<String> {
    let trimmed = lang.trim();
    if trimmed.is_empty() {
        return Ok("en".to_string());
    }
    let key = trimmed.to_ascii_lowercase().replace('_', "-");
    match key.as_str() {
        "en" | "en-us" => Ok("en".to_string()),
        _ => Err(UserError::Other {
            message: format!(
                "unsupported TTS language '{trimmed}'\n  \
                 Hint: KittenTTS G2P is English-only in this release; use en or en-US."
            ),
        }
        .into()),
    }
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

/// Trim and enforce max chars with complete-or-error semantics.
///
/// Oversized input is a user error. Silent truncation is not supported; raise
/// `[tts].max_chars` only when intentionally accepting larger inputs (chunking
/// still enforces model phoneme limits).
pub fn prepare_text(text: &str, max_chars: usize) -> Result<PreparedText> {
    validate_text(text)?;
    let trimmed = text.trim();
    let char_count = trimmed.chars().count();
    if char_count > max_chars {
        return Err(UserError::Other {
            message: format!(
                "TTS text is too long ({char_count} characters; limit is {max_chars}).\n  \
                 Hint: shorten the input, split it yourself, or raise [tts].max_chars in config. \
                 Aurum does not silently truncate synthesis input."
            ),
        }
        .into());
    }
    Ok(PreparedText {
        text: trimmed.to_string(),
        text_chars: char_count,
        text_truncated: false,
    })
}

/// Reject a requested sample-rate override that is not the adapter's native rate.
///
/// Real resampling is not implemented; metadata-only relabeling is refused.
pub fn resolve_sample_rate(requested: Option<u32>, native_hz: u32) -> Result<u32> {
    match requested {
        None => Ok(native_hz),
        Some(hz) if hz == native_hz => Ok(native_hz),
        Some(hz) => Err(UserError::Other {
            message: format!(
                "unsupported TTS sample rate {hz} Hz (model native rate is {native_hz} Hz).\n  \
                 Hint: omit sample_rate_hz to use the native rate. Real resampling is not \
                 available in this release."
            ),
        }
        .into()),
    }
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
    fn prepare_rejects_over_limit() {
        let long = "word ".repeat(20);
        let err = prepare_text(&long, 30).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("too long"));
    }

    #[test]
    fn prepare_no_trunc_under_limit() {
        let p = prepare_text("hello", 100).unwrap();
        assert!(!p.text_truncated);
        assert_eq!(p.text, "hello");
        assert_eq!(p.text_chars, 5);
    }

    #[test]
    fn sample_rate_reject_non_native() {
        assert_eq!(resolve_sample_rate(None, 24_000).unwrap(), 24_000);
        assert_eq!(resolve_sample_rate(Some(24_000), 24_000).unwrap(), 24_000);
        let err = resolve_sample_rate(Some(16_000), 24_000).unwrap_err();
        assert_eq!(err.exit_code(), 2);
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

    #[test]
    fn language_accepts_english_aliases() {
        assert_eq!(normalize_tts_language("").unwrap(), "en");
        assert_eq!(normalize_tts_language("en").unwrap(), "en");
        assert_eq!(normalize_tts_language("en-US").unwrap(), "en");
        assert_eq!(normalize_tts_language("en_us").unwrap(), "en");
        assert_eq!(normalize_tts_language(" EN-us ").unwrap(), "en");
    }

    #[test]
    fn language_rejects_unsupported() {
        for bad in ["fr", "de", "es-ES", "zh", "en-GB", "ja"] {
            let err = normalize_tts_language(bad).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{bad}");
        }
    }

    #[test]
    fn input_byte_budget_scales_with_max_chars() {
        assert_eq!(tts_input_byte_budget(0), READ_BYTE_OVERHEAD);
        assert_eq!(
            tts_input_byte_budget(100),
            100 * READ_BYTES_PER_CHAR + READ_BYTE_OVERHEAD
        );
        assert!(tts_input_byte_budget(5_000) < 30_000 + READ_BYTE_OVERHEAD);
    }
}
