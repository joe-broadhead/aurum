//! Half-duplex turn machine (no providers).

use super::LiveSessionConfig;
use crate::error::{Result, UserError};
use crate::pcm::PcmBuffer;

/// Conversation phase. Agent playback is [`Self::Speaking`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePhase {
    Listening,
    UserSpeaking,
    /// Utterance frozen; waiting for STT commit.
    Ending,
    /// User turn committed; waiting for agent audio.
    Thinking,
    Speaking,
    Closed,
}

pub(crate) struct LiveTurn {
    cfg: LiveSessionConfig,
    phase: LivePhase,
    buf: PcmBuffer,
    utterance: Vec<f32>,
    speech_samples: usize,
    silence_samples: usize,
    ignored_pending: usize,
    ignored_total: usize,
}

impl LiveTurn {
    pub(crate) fn new(cfg: LiveSessionConfig) -> Self {
        let max_secs = cfg.max_utterance_secs;
        Self {
            cfg,
            phase: LivePhase::Listening,
            buf: PcmBuffer::bounded(max_secs),
            utterance: Vec::new(),
            speech_samples: 0,
            silence_samples: 0,
            ignored_pending: 0,
            ignored_total: 0,
        }
    }

    pub(crate) fn phase(&self) -> LivePhase {
        self.phase
    }

    pub(crate) fn ignored_total(&self) -> usize {
        self.ignored_total
    }

    pub(crate) fn take_ignored_pending(&mut self) -> usize {
        let n = self.ignored_pending;
        self.ignored_pending = 0;
        n
    }

    pub(crate) fn buffered_samples(&self) -> usize {
        self.buf.len()
    }

    /// Push 16 kHz mono f32. Returns true on Listening → UserSpeaking.
    pub(crate) fn push_pcm(&mut self, chunk: &[f32]) -> Result<bool> {
        self.ensure_not_closed()?;
        if chunk.is_empty() {
            return Ok(false);
        }
        match self.phase {
            LivePhase::Speaking => {
                self.ignored_pending = self.ignored_pending.saturating_add(chunk.len());
                self.ignored_total = self.ignored_total.saturating_add(chunk.len());
                return Ok(false);
            }
            LivePhase::Ending | LivePhase::Thinking | LivePhase::Closed => {
                return Ok(false);
            }
            LivePhase::Listening | LivePhase::UserSpeaking => {}
        }

        self.buf.push(chunk)?;
        let rms = chunk_rms(chunk);
        let speech = rms >= self.cfg.min_rms;
        let mut became_speaking = false;

        if speech {
            if self.phase == LivePhase::Listening {
                self.phase = LivePhase::UserSpeaking;
                became_speaking = true;
            }
            self.speech_samples = self.speech_samples.saturating_add(chunk.len());
            self.silence_samples = 0;
        } else if self.phase == LivePhase::UserSpeaking {
            self.silence_samples = self.silence_samples.saturating_add(chunk.len());
            if self.speech_samples >= self.cfg.min_speech_samples()
                && self.silence_samples >= self.cfg.trailing_silence_samples()
            {
                self.freeze_utterance()?;
            }
        }
        Ok(became_speaking)
    }

    /// Explicit endpoint (replay EOF). Bypasses min-speech / trailing-silence.
    pub(crate) fn end_user_turn(&mut self) -> Result<()> {
        self.ensure_not_closed()?;
        match self.phase {
            LivePhase::Listening | LivePhase::UserSpeaking => self.freeze_utterance(),
            LivePhase::Ending => Ok(()),
            other => Err(UserError::Other {
                message: format!(
                    "end_user_turn is not valid in phase {other:?} (expected Listening, UserSpeaking, or Ending)"
                ),
            }
            .into()),
        }
    }

    pub(crate) fn take_utterance(&mut self) -> Result<Vec<f32>> {
        self.ensure_not_closed()?;
        if self.phase != LivePhase::Ending {
            return Err(UserError::Other {
                message: format!(
                    "take_utterance requires Ending (got {:?}); call end_user_turn first",
                    self.phase
                ),
            }
            .into());
        }
        if self.utterance.is_empty() {
            return Err(UserError::InvalidAudio {
                reason: "user turn has no PCM (silence-only or empty input)".into(),
            }
            .into());
        }
        Ok(std::mem::take(&mut self.utterance))
    }

    pub(crate) fn enter_thinking(&mut self) -> Result<()> {
        self.ensure_not_closed()?;
        if self.phase != LivePhase::Ending {
            return Err(UserError::Other {
                message: format!("enter_thinking requires Ending (got {:?})", self.phase),
            }
            .into());
        }
        self.phase = LivePhase::Thinking;
        Ok(())
    }

    pub(crate) fn begin_speaking(&mut self) -> Result<()> {
        self.ensure_not_closed()?;
        match self.phase {
            LivePhase::Thinking | LivePhase::Speaking => {
                self.phase = LivePhase::Speaking;
                Ok(())
            }
            other => Err(UserError::Other {
                message: format!(
                    "speak is not valid in phase {other:?} (expected Thinking or Speaking)"
                ),
            }
            .into()),
        }
    }

    pub(crate) fn end_agent_turn(&mut self) -> Result<()> {
        self.ensure_not_closed()?;
        if self.phase != LivePhase::Speaking && self.phase != LivePhase::Thinking {
            return Err(UserError::Other {
                message: format!(
                    "end_agent_turn requires Thinking or Speaking (got {:?})",
                    self.phase
                ),
            }
            .into());
        }
        self.reset_listen();
        Ok(())
    }

    pub(crate) fn close(&mut self) {
        self.phase = LivePhase::Closed;
        self.buf.clear();
        self.utterance.clear();
    }

    fn freeze_utterance(&mut self) -> Result<()> {
        let mut samples = Vec::new();
        self.buf.copy_samples_into(&mut samples);
        if samples.is_empty() {
            return Err(UserError::InvalidAudio {
                reason: "user turn has no PCM (silence-only or empty input)".into(),
            }
            .into());
        }
        self.utterance = samples;
        self.buf.clear();
        self.speech_samples = 0;
        self.silence_samples = 0;
        self.phase = LivePhase::Ending;
        Ok(())
    }

    fn reset_listen(&mut self) {
        self.phase = LivePhase::Listening;
        self.buf.clear();
        self.utterance.clear();
        self.speech_samples = 0;
        self.silence_samples = 0;
        self.ignored_pending = 0;
        // Keep ignored_total for diagnostics across turns.
    }

    fn ensure_not_closed(&self) -> Result<()> {
        if self.phase == LivePhase::Closed {
            return Err(UserError::Other {
                message: "live session is closed".into(),
            }
            .into());
        }
        Ok(())
    }
}

fn chunk_rms(chunk: &[f32]) -> f32 {
    if chunk.is_empty() {
        return 0.0;
    }
    let sum: f64 = chunk.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / chunk.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_fast() -> LiveSessionConfig {
        LiveSessionConfig {
            max_utterance_secs: 2.0,
            min_speech_secs: 0.05,
            trailing_silence_secs: 0.05,
            min_rms: 0.1,
            max_speak_ahead: 2,
        }
    }

    #[test]
    fn energy_endpoint_after_speech_then_silence() {
        let mut t = LiveTurn::new(cfg_fast());
        let speech = vec![0.5f32; 800];
        let silence = vec![0.0f32; 800];
        assert!(t.push_pcm(&speech).unwrap());
        assert_eq!(t.phase(), LivePhase::UserSpeaking);
        t.push_pcm(&silence).unwrap();
        assert_eq!(t.phase(), LivePhase::Ending);
        let u = t.take_utterance().unwrap();
        assert_eq!(u.len(), 1600);
    }

    #[test]
    fn explicit_end_bypasses_silence() {
        let mut t = LiveTurn::new(cfg_fast());
        t.push_pcm(&[0.4; 100]).unwrap();
        t.end_user_turn().unwrap();
        assert_eq!(t.phase(), LivePhase::Ending);
        assert_eq!(t.take_utterance().unwrap().len(), 100);
    }

    #[test]
    fn half_duplex_ignores_pcm_while_speaking() {
        let mut t = LiveTurn::new(cfg_fast());
        t.push_pcm(&[0.4; 800]).unwrap();
        t.end_user_turn().unwrap();
        let _ = t.take_utterance().unwrap();
        t.enter_thinking().unwrap();
        t.begin_speaking().unwrap();
        t.push_pcm(&[0.9; 400]).unwrap();
        assert_eq!(t.ignored_total(), 400);
        assert_eq!(t.buffered_samples(), 0);
        t.end_agent_turn().unwrap();
        assert_eq!(t.phase(), LivePhase::Listening);
    }

    #[test]
    fn empty_end_is_user_error() {
        let mut t = LiveTurn::new(cfg_fast());
        let err = t.end_user_turn().unwrap_err();
        assert!(err.to_string().contains("no PCM"), "{err}");
    }

    #[test]
    fn overflow_bounded_buffer() {
        let cfg = LiveSessionConfig {
            max_utterance_secs: 0.01, // 160 samples
            ..cfg_fast()
        };
        let mut t = LiveTurn::new(cfg);
        let err = t.push_pcm(&[0.2; 200]).unwrap_err();
        assert!(err.to_string().contains("too long") || err.to_string().contains("Audio"));
    }
}
