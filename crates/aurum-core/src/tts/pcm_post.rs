//! Signal-aware trailing-silence handling and final PCM validation for TTS.
//!
//! Replaces the previous unconditional `TAIL_TRIM = 2000` samples, which could
//! clip short valid utterances or empty the buffer entirely.

use crate::error::{ProviderError, Result};

/// Peak amplitude limit applied before i16 quantization (shared with synthesis).
pub const PEAK_LIMIT: f32 = 0.95;

/// Default trailing-silence policy for KittenTTS mono 24 kHz output.
#[derive(Debug, Clone, Copy)]
pub struct TailTrimPolicy {
    /// |sample| below this absolute level counts as silence (f32 domain).
    pub energy_threshold: f32,
    /// Minimum contiguous silent samples at the tail before trim engages.
    pub min_silence_samples: usize,
    /// Samples of silence retained after the last audible sample.
    pub retain_padding_samples: usize,
    /// Never trim below this many samples (guards short valid clips).
    pub min_retained_samples: usize,
}

impl Default for TailTrimPolicy {
    fn default() -> Self {
        // ~24 kHz defaults: ~40 ms silence window, ~20 ms padding, ≥ 20 ms audio.
        Self {
            energy_threshold: 0.008,
            min_silence_samples: 960,
            retain_padding_samples: 480,
            min_retained_samples: 480,
        }
    }
}

/// Remove trailing silence when a sustained low-energy region is detected.
///
/// If confidence is insufficient (no sustained silence, or trim would empty /
/// undersize the buffer), the original slice is returned unchanged — never
/// destroy valid short output.
pub fn trim_trailing_silence(samples: &[f32], policy: TailTrimPolicy) -> &[f32] {
    if samples.is_empty() {
        return samples;
    }

    // Walk from the end to find the last sample above the energy threshold.
    let mut last_audible = None;
    for (i, s) in samples.iter().enumerate().rev() {
        let v = if s.is_finite() { s.abs() } else { 0.0 };
        if v > policy.energy_threshold {
            last_audible = Some(i);
            break;
        }
    }

    let Some(last) = last_audible else {
        // Entire buffer is below threshold — do not destroy it; caller validates.
        return samples;
    };

    let silence_len = samples.len().saturating_sub(last + 1);
    if silence_len < policy.min_silence_samples {
        // Not enough sustained silence to justify a destructive trim.
        return samples;
    }

    let end = (last + 1)
        .saturating_add(policy.retain_padding_samples)
        .min(samples.len());
    if end < policy.min_retained_samples {
        return samples;
    }
    if end >= samples.len() {
        return samples;
    }
    &samples[..end]
}

/// Validate final f32 PCM before quantization.
pub fn validate_raw_pcm(samples: &[f32], sample_rate_hz: u32) -> Result<()> {
    if samples.is_empty() {
        return Err(ProviderError::Other {
            message: "TTS model produced empty audio buffer".into(),
        }
        .into());
    }
    if sample_rate_hz == 0 {
        return Err(ProviderError::Other {
            message: "TTS sample rate is zero".into(),
        }
        .into());
    }

    let mut non_finite = 0usize;
    let mut peak = 0.0f32;
    for s in samples {
        if !s.is_finite() {
            non_finite += 1;
            continue;
        }
        peak = peak.max(s.abs());
    }
    // Tolerate sparse non-finite values (replaced with 0 at quantize); reject all-garbage.
    if non_finite == samples.len() {
        return Err(ProviderError::Other {
            message: "TTS model produced non-finite audio samples only".into(),
        }
        .into());
    }
    if peak > 100.0 {
        return Err(ProviderError::Other {
            message: format!("TTS model produced implausible peak amplitude ({peak})"),
        }
        .into());
    }

    // Minimum duration ~5 ms — below this is almost certainly a bad path.
    let min_samples = (sample_rate_hz as usize / 200).max(1);
    if samples.len() < min_samples {
        return Err(ProviderError::Other {
            message: format!(
                "TTS audio too short ({} samples at {} Hz; minimum {min_samples})",
                samples.len(),
                sample_rate_hz
            ),
        }
        .into());
    }

    // Soft upper bound: 2 hours of mono audio (guards runaway chunk concat).
    let max_samples = sample_rate_hz as usize * 7_200;
    if samples.len() > max_samples {
        return Err(ProviderError::Other {
            message: format!(
                "TTS audio exceeds duration bound ({} samples at {} Hz)",
                samples.len(),
                sample_rate_hz
            ),
        }
        .into());
    }
    Ok(())
}

/// Duration in milliseconds from final sample count and actual rate.
pub fn duration_ms_from_pcm(sample_count: usize, sample_rate_hz: u32) -> u64 {
    if sample_rate_hz == 0 {
        return 0;
    }
    (sample_count as u64)
        .saturating_mul(1000)
        .checked_div(sample_rate_hz as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| amp * ((i as f32) * 0.1).sin()).collect()
    }

    #[test]
    fn short_valid_output_not_emptied() {
        let samples = tone(300, 0.2);
        let policy = TailTrimPolicy {
            min_silence_samples: 1000,
            min_retained_samples: 100,
            ..Default::default()
        };
        let out = trim_trailing_silence(&samples, policy);
        assert_eq!(out.len(), samples.len());
        assert!(!out.is_empty());
    }

    #[test]
    fn long_trailing_silence_is_trimmed_with_padding() {
        let mut samples = tone(2000, 0.3);
        samples.extend(std::iter::repeat_n(0.0f32, 5000));
        let policy = TailTrimPolicy {
            energy_threshold: 0.01,
            min_silence_samples: 1000,
            retain_padding_samples: 200,
            min_retained_samples: 100,
        };
        let out = trim_trailing_silence(&samples, policy);
        assert!(out.len() < samples.len());
        // Last audible around index 1999 + 200 padding.
        assert!(out.len() >= 2000);
        assert!(out.len() <= 2200 + 50);
        assert!(!out.is_empty());
    }

    #[test]
    fn plosive_ending_not_clipped_without_sustained_silence() {
        // Speech-like then short quiet tail (< min_silence).
        let mut samples = tone(4000, 0.4);
        samples.push(0.5); // strong final sample
        samples.extend(std::iter::repeat_n(0.0f32, 100));
        let policy = TailTrimPolicy::default();
        let out = trim_trailing_silence(&samples, policy);
        assert_eq!(out.len(), samples.len());
        assert!((out.last().copied().unwrap_or(1.0)).abs() < 0.01 || out.len() == samples.len());
        // Final audible peak must still be present.
        assert!(out.iter().any(|s| *s > 0.3));
    }

    #[test]
    fn all_silence_not_destroyed() {
        let samples = vec![0.0f32; 2000];
        let out = trim_trailing_silence(&samples, TailTrimPolicy::default());
        assert_eq!(out.len(), 2000);
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_raw_pcm(&[], 24_000).is_err());
    }

    #[test]
    fn validate_accepts_normal() {
        let s = tone(2400, 0.2);
        validate_raw_pcm(&s, 24_000).unwrap();
    }

    #[test]
    fn duration_rounding() {
        assert_eq!(duration_ms_from_pcm(24_000, 24_000), 1000);
        assert_eq!(duration_ms_from_pcm(12_000, 24_000), 500);
        assert_eq!(duration_ms_from_pcm(1, 24_000), 0);
    }
}
