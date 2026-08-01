//! Validated domain primitives (JOE-1786).
//!
//! These wrappers make invalid sample rates / durations unrepresentable at the
//! type level for new construction paths. Existing public structs may still
//! expose raw fields during the 0.0.x transition; prefer these constructors.

use crate::audio::WHISPER_SAMPLE_RATE;
use crate::error::{Result, UserError};

/// Sample rate in Hz that has been checked for non-zero / finite-ish range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SampleRateHz(u32);

impl SampleRateHz {
    /// Accept any positive rate (remote paths may differ from whisper).
    pub fn try_new(hz: u32) -> Result<Self> {
        if hz == 0 {
            return Err(UserError::Other {
                message: "sample rate must be positive".into(),
            }
            .into());
        }
        Ok(Self(hz))
    }

    /// Whisper / local STT required rate.
    pub fn whisper() -> Self {
        Self(WHISPER_SAMPLE_RATE)
    }

    pub fn get(self) -> u32 {
        self.0
    }

    pub fn is_whisper(self) -> bool {
        self.0 == WHISPER_SAMPLE_RATE
    }
}

/// Non-negative finite duration in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FiniteDurationSecs(f64);

impl FiniteDurationSecs {
    pub fn try_new(secs: f64) -> Result<Self> {
        if !secs.is_finite() || secs < 0.0 {
            return Err(UserError::Other {
                message: format!("duration must be finite and non-negative (got {secs})"),
            }
            .into());
        }
        Ok(Self(secs))
    }

    pub fn get(self) -> f64 {
        self.0
    }

    pub fn zero() -> Self {
        Self(0.0)
    }
}

/// Provider / model identifier: non-empty, no control characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelId(String);

impl ModelId {
    pub fn try_new(id: impl Into<String>) -> Result<Self> {
        let s = id.into();
        let t = s.trim();
        if t.is_empty() {
            return Err(UserError::Other {
                message: "model id must be non-empty".into(),
            }
            .into());
        }
        if t.chars().any(|c| c.is_control()) {
            return Err(UserError::Other {
                message: "model id must not contain control characters".into(),
            }
            .into());
        }
        Ok(Self(t.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_rejects_zero() {
        assert!(SampleRateHz::try_new(0).is_err());
        assert_eq!(SampleRateHz::whisper().get(), WHISPER_SAMPLE_RATE);
    }

    #[test]
    fn sample_rate_is_whisper_only_for_16000() {
        // Kills mutants that flip is_whisper to always-true or invert equality.
        assert!(SampleRateHz::whisper().is_whisper());
        assert!(SampleRateHz::try_new(WHISPER_SAMPLE_RATE)
            .unwrap()
            .is_whisper());
        assert!(!SampleRateHz::try_new(8_000).unwrap().is_whisper());
        assert!(!SampleRateHz::try_new(44_100).unwrap().is_whisper());
    }

    #[test]
    fn duration_rejects_nan() {
        assert!(FiniteDurationSecs::try_new(f64::NAN).is_err());
        assert!(FiniteDurationSecs::try_new(-1.0).is_err());
        assert_eq!(FiniteDurationSecs::try_new(1.5).unwrap().get(), 1.5);
    }

    #[test]
    fn model_id_rejects_empty_and_control() {
        assert!(ModelId::try_new("").is_err());
        assert!(ModelId::try_new("  ").is_err());
        assert!(ModelId::try_new("a\nb").is_err());
        assert_eq!(ModelId::try_new("tiny-q5_1").unwrap().as_str(), "tiny-q5_1");
    }

    #[test]
    fn model_id_into_inner_preserves_value() {
        // Kills mutants that replace into_inner with a constant string.
        let id = ModelId::try_new("tiny-q5_1").unwrap();
        assert_eq!(id.into_inner(), "tiny-q5_1");
    }
}
