//! Turn-based, half-duplex conversation session (GitHub #140).
//!
//! Hosts push 16 kHz mono f32 and receive [`LiveEvent::UserFinal`] / [`LiveEvent::AgentPcm`].
//! Aurum does **not** own the microphone or a streaming decoder. Remote STT is one-shot
//! at endpoint; inbound PCM is ignored while the agent is speaking.
//!
//! Construct from an [`crate::AurumEngine`] so STT/TTS route through the provider
//! registry (ADR-002 / JOE-1938). Session policy lives here; provider ids stay on
//! engine config (`[stt]` / `[tts]`).

mod session;
mod turn;

pub use session::LiveSession;
pub use turn::LivePhase;

use crate::audio::WHISPER_SAMPLE_RATE;
use crate::error::{Result, UserError};

/// Session policy only — not a second provider config.
///
/// Provider ids, models, and secrets remain on [`crate::sdk::AurumConfig`] /
/// [`crate::AurumEngine`].
#[derive(Debug, Clone)]
pub struct LiveSessionConfig {
    /// Maximum user-utterance length in seconds (non-rolling; overflow is a user error).
    pub max_utterance_secs: f64,
    /// Minimum speech duration before energy-based endpoint may fire.
    pub min_speech_secs: f64,
    /// Trailing near-silence that ends a user turn (energy path only).
    pub trailing_silence_secs: f64,
    /// RMS gate for “speech” vs “silence” (chunk-level).
    pub min_rms: f32,
    /// Max unconsumed [`LiveEvent::AgentPcm`] units (speak-ahead cap).
    pub max_speak_ahead: usize,
}

impl Default for LiveSessionConfig {
    fn default() -> Self {
        Self {
            max_utterance_secs: 30.0,
            min_speech_secs: 0.25,
            trailing_silence_secs: 0.6,
            min_rms: 0.01,
            max_speak_ahead: 2,
        }
    }
}

impl LiveSessionConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.max_utterance_secs.is_finite() || self.max_utterance_secs <= 0.0 {
            return Err(UserError::InvalidConfig {
                reason: "live max_utterance_secs must be finite and > 0".into(),
            }
            .into());
        }
        if !self.min_speech_secs.is_finite() || self.min_speech_secs < 0.0 {
            return Err(UserError::InvalidConfig {
                reason: "live min_speech_secs must be finite and >= 0".into(),
            }
            .into());
        }
        if !self.trailing_silence_secs.is_finite() || self.trailing_silence_secs < 0.0 {
            return Err(UserError::InvalidConfig {
                reason: "live trailing_silence_secs must be finite and >= 0".into(),
            }
            .into());
        }
        if !self.min_rms.is_finite() || self.min_rms < 0.0 {
            return Err(UserError::InvalidConfig {
                reason: "live min_rms must be finite and >= 0".into(),
            }
            .into());
        }
        if self.max_speak_ahead == 0 {
            return Err(UserError::InvalidConfig {
                reason: "live max_speak_ahead must be >= 1".into(),
            }
            .into());
        }
        Ok(())
    }

    pub(crate) fn min_speech_samples(&self) -> usize {
        secs_to_samples(self.min_speech_secs)
    }

    pub(crate) fn trailing_silence_samples(&self) -> usize {
        secs_to_samples(self.trailing_silence_secs)
    }
}

fn secs_to_samples(secs: f64) -> usize {
    (secs.max(0.0) * f64::from(WHISPER_SAMPLE_RATE)).round() as usize
}

/// Events drained via [`LiveSession::poll`].
#[derive(Debug, Clone)]
pub enum LiveEvent {
    /// Nothing pending.
    Idle,
    /// First energy-gated speech of a user turn.
    UserSpeaking { samples: usize },
    /// Endpoint committed; `text` is `result.text()` (final ASR, not a partial).
    UserFinal {
        text: String,
        result: crate::providers::TranscriptionResult,
    },
    /// One synthesized agent unit ready for the host to play.
    AgentPcm { result: crate::tts::SynthesisResult },
    /// PCM arrived while the agent turn is in flight and was not buffered.
    IgnoredWhileSpeaking { samples: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_validates() {
        LiveSessionConfig::default().validate().unwrap();
    }

    #[test]
    fn config_rejects_zero_speak_ahead() {
        let c = LiveSessionConfig {
            max_speak_ahead: 0,
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn config_rejects_nan_rms() {
        let c = LiveSessionConfig {
            min_rms: f32::NAN,
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }
}
