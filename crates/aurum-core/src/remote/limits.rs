//! Response body and transcript expansion bounds (JOE-1588).

use crate::error::{ProviderError, Result};
use crate::providers::Segment;
use futures_util::StreamExt;
use reqwest::Response;

/// Default hard caps for response bodies (bytes).
pub const DEFAULT_STT_BODY_CAP: usize = 8 * 1024 * 1024;
pub const DEFAULT_CHAT_BODY_CAP: usize = 4 * 1024 * 1024;
pub const DEFAULT_CLEANUP_BODY_CAP: usize = 2 * 1024 * 1024;

/// Limits applied after JSON parse to transcript structures.
#[derive(Debug, Clone, Copy)]
pub struct TranscriptLimits {
    pub max_text_chars: usize,
    pub max_segments: usize,
    pub max_segment_chars: usize,
    pub max_total_segment_chars: usize,
    /// Reject output text that expands beyond this multiple of input size.
    pub max_expansion_ratio: f64,
}

impl Default for TranscriptLimits {
    fn default() -> Self {
        Self {
            max_text_chars: 500_000,
            max_segments: 10_000,
            max_segment_chars: 8_000,
            max_total_segment_chars: 1_000_000,
            max_expansion_ratio: 8.0,
        }
    }
}

/// Endpoint-specific body byte caps.
#[derive(Debug, Clone, Copy)]
pub struct RemoteBodyLimits {
    pub max_bytes: usize,
}

impl RemoteBodyLimits {
    pub fn stt() -> Self {
        Self {
            max_bytes: DEFAULT_STT_BODY_CAP,
        }
    }
    pub fn chat() -> Self {
        Self {
            max_bytes: DEFAULT_CHAT_BODY_CAP,
        }
    }
    pub fn cleanup() -> Self {
        Self {
            max_bytes: DEFAULT_CLEANUP_BODY_CAP,
        }
    }
}

/// Stream a response body under a hard byte cap.
///
/// `Content-Length` is an early rejection check only; it never raises the cap.
pub async fn read_body_limited(
    response: Response,
    provider: &str,
    limits: RemoteBodyLimits,
) -> Result<Vec<u8>> {
    if let Some(cl) = response.content_length() {
        if cl as usize > limits.max_bytes {
            return Err(ProviderError::ResponseTooLarge {
                provider: provider.into(),
                reason: format!("Content-Length {cl} exceeds cap {}", limits.max_bytes),
            }
            .into());
        }
    }

    let mut out = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ProviderError::Network {
            provider: provider.into(),
            reason: e.to_string(),
        })?;
        if out.len().saturating_add(chunk.len()) > limits.max_bytes {
            return Err(ProviderError::ResponseTooLarge {
                provider: provider.into(),
                reason: format!("stream exceeded body cap of {} bytes", limits.max_bytes),
            }
            .into());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Validate transcript text size and optional expansion vs input.
pub fn validate_text_bounds(
    text: &str,
    input_chars: Option<usize>,
    limits: TranscriptLimits,
    provider: &str,
) -> Result<()> {
    let n = text.chars().count();
    if n > limits.max_text_chars {
        return Err(ProviderError::LimitExceeded {
            reason: format!(
                "transcript text has {n} chars (limit {})",
                limits.max_text_chars
            ),
        }
        .into());
    }
    if let Some(input) = input_chars {
        if input > 0 {
            let ratio = n as f64 / input as f64;
            if ratio > limits.max_expansion_ratio {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: format!(
                        "implausible expansion ratio {ratio:.1}x (limit {}x)",
                        limits.max_expansion_ratio
                    ),
                }
                .into());
            }
        }
    }
    Ok(())
}

/// Validate segment list: counts, lengths, finite ordered timestamps.
pub fn validate_segments(
    segments: &[Segment],
    media_duration: f64,
    limits: TranscriptLimits,
    provider: &str,
) -> Result<()> {
    if segments.len() > limits.max_segments {
        return Err(ProviderError::LimitExceeded {
            reason: format!(
                "{} segments exceeds limit {}",
                segments.len(),
                limits.max_segments
            ),
        }
        .into());
    }
    let mut total_chars = 0usize;
    let mut prev_end = 0.0f64;
    for (i, seg) in segments.iter().enumerate() {
        if !seg.start().is_finite() || !seg.end().is_finite() {
            return Err(ProviderError::InvalidProviderPayload {
                provider: provider.into(),
                reason: format!("segment {i} has non-finite timestamps"),
            }
            .into());
        }
        if seg.start() < 0.0 || seg.end() < 0.0 {
            return Err(ProviderError::InvalidProviderPayload {
                provider: provider.into(),
                reason: format!("segment {i} has negative timestamps"),
            }
            .into());
        }
        if seg.end() < seg.start() {
            return Err(ProviderError::InvalidProviderPayload {
                provider: provider.into(),
                reason: format!("segment {i} end < start"),
            }
            .into());
        }
        // Soft bound: allow small overshoot past media duration.
        let bound = if media_duration.is_finite() && media_duration > 0.0 {
            media_duration + 1.0
        } else {
            f64::MAX
        };
        if seg.start() > bound || seg.end() > bound {
            return Err(ProviderError::InvalidProviderPayload {
                provider: provider.into(),
                reason: format!("segment {i} timestamps exceed media duration"),
            }
            .into());
        }
        // Ordered with limited overlap (start may equal previous end).
        if seg.start() + 0.001 < prev_end - 30.0 {
            // Allow modest overlap but reject severe reordering.
            return Err(ProviderError::InvalidProviderPayload {
                provider: provider.into(),
                reason: format!("segment {i} severely out of order"),
            }
            .into());
        }
        prev_end = seg.end().max(prev_end);

        let sc = seg.text().chars().count();
        if sc > limits.max_segment_chars {
            return Err(ProviderError::LimitExceeded {
                reason: format!(
                    "segment {i} has {sc} chars (limit {})",
                    limits.max_segment_chars
                ),
            }
            .into());
        }
        total_chars = total_chars.saturating_add(sc);
        if total_chars > limits.max_total_segment_chars {
            return Err(ProviderError::LimitExceeded {
                reason: format!(
                    "total segment text exceeds {} chars",
                    limits.max_total_segment_chars
                ),
            }
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_expansion() {
        let err = validate_text_bounds(
            "x".repeat(100).as_str(),
            Some(5),
            TranscriptLimits::default(),
            "t",
        )
        .unwrap_err();
        assert!(err.to_string().contains("expansion") || err.to_string().contains("limit"));
    }

    #[test]
    fn rejects_nonfinite_segment() {
        let segs = vec![Segment::from_parts_unchecked(
            f64::NAN,
            1.0,
            "hi".to_string(),
        )];
        assert!(validate_segments(&segs, 10.0, TranscriptLimits::default(), "t").is_err());
    }

    #[test]
    fn accepts_normal_segments() {
        let segs = vec![
            Segment::from_parts_unchecked(0.0, 1.0, "a".to_string()),
            Segment::from_parts_unchecked(1.0, 2.0, "b".to_string()),
        ];
        validate_segments(&segs, 2.0, TranscriptLimits::default(), "t").unwrap();
    }
}
