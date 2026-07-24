//! In-memory PCM buffer for mic / streaming hosts (no files, no ffmpeg).
//!
//! This is the clean embedder surface: push float chunks, then finalize into
//! an [`AudioInput`](crate::audio::AudioInput) for any provider.

use crate::audio::{AudioInput, WHISPER_SAMPLE_RATE};
use crate::error::{Result, UserError};
use std::sync::Arc;

/// Default max buffer for interactive dictation (~60 s @ 16 kHz), matching
/// common hold-to-talk products. Longer content should use file transcription.
pub const DEFAULT_DICTATION_MAX_SECS: f64 = 60.0;

/// Rolling / accumulating mono PCM buffer at [`WHISPER_SAMPLE_RATE`].
#[derive(Debug, Clone)]
pub struct PcmBuffer {
    samples: Vec<f32>,
    max_samples: usize,
    /// When true, overflow drops oldest samples (rolling window).
    /// When false, [`push`](Self::push) errors on overflow.
    rolling: bool,
}

impl PcmBuffer {
    /// Create a buffer capped at `max_secs` of audio (rolling by default).
    pub fn with_max_secs(max_secs: f64) -> Self {
        let max_samples = (max_secs.max(0.0) * f64::from(WHISPER_SAMPLE_RATE)).round() as usize;
        Self {
            samples: Vec::with_capacity(max_samples.clamp(1, 16_000)),
            max_samples: max_samples.max(1),
            rolling: true,
        }
    }

    /// Dictation-oriented default: ~60 s rolling window.
    pub fn dictation() -> Self {
        Self::with_max_secs(DEFAULT_DICTATION_MAX_SECS)
    }

    /// Grow until `max_secs`, then error instead of dropping audio.
    pub fn bounded(max_secs: f64) -> Self {
        let mut b = Self::with_max_secs(max_secs);
        b.rolling = false;
        b
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn max_samples(&self) -> usize {
        self.max_samples
    }

    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / f64::from(WHISPER_SAMPLE_RATE)
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Append mono f32 samples already at 16 kHz.
    pub fn push(&mut self, chunk: &[f32]) -> Result<()> {
        if chunk.is_empty() {
            return Ok(());
        }
        if self.rolling {
            self.samples.extend_from_slice(chunk);
            if self.samples.len() > self.max_samples {
                let overflow = self.samples.len() - self.max_samples;
                self.samples.drain(0..overflow);
            }
            Ok(())
        } else if self.samples.len().saturating_add(chunk.len()) > self.max_samples {
            Err(UserError::AudioTooLong {
                duration_secs: (self.samples.len() + chunk.len()) as f64
                    / f64::from(WHISPER_SAMPLE_RATE),
                max_secs: self.max_samples as f64 / f64::from(WHISPER_SAMPLE_RATE),
            }
            .into())
        } else {
            self.samples.extend_from_slice(chunk);
            Ok(())
        }
    }

    /// Simple RMS energy (for silence / partial gates in the host).
    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.samples.iter().map(|s| s * s).sum();
        (sum / self.samples.len() as f32).sqrt()
    }

    /// Snapshot as [`AudioInput`] without clearing the buffer.
    pub fn to_audio_input(&self) -> Result<AudioInput> {
        if self.samples.is_empty() {
            return Err(UserError::InvalidAudio {
                reason: "PCM buffer is empty".into(),
            }
            .into());
        }
        let samples: Arc<[f32]> = self.samples.clone().into();
        AudioInput::from_pcm(samples, WHISPER_SAMPLE_RATE)
    }

    /// Take ownership of samples and clear the buffer.
    pub fn take_audio_input(&mut self) -> Result<AudioInput> {
        if self.samples.is_empty() {
            return Err(UserError::InvalidAudio {
                reason: "PCM buffer is empty".into(),
            }
            .into());
        }
        let samples: Arc<[f32]> = std::mem::take(&mut self.samples).into();
        AudioInput::from_pcm(samples, WHISPER_SAMPLE_RATE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_drops_oldest() {
        let mut buf = PcmBuffer::with_max_secs(0.01); // 160 samples
        assert_eq!(buf.max_samples(), 160);
        buf.push(&vec![1.0; 100]).unwrap();
        buf.push(&vec![2.0; 100]).unwrap();
        assert_eq!(buf.len(), 160);
        assert!(buf.samples().iter().all(|&s| s == 1.0 || s == 2.0));
        assert!(buf.samples()[0] == 2.0 || buf.samples().contains(&2.0));
        // oldest 40 of the ones should be gone → first sample should be 1.0 still or 2.0
        assert_eq!(buf.samples()[0], 1.0);
        assert_eq!(buf.samples()[59], 1.0);
        assert_eq!(buf.samples()[60], 2.0);
    }

    #[test]
    fn bounded_errors() {
        let mut buf = PcmBuffer::bounded(0.01);
        buf.push(&vec![0.1; 160]).unwrap();
        let err = buf.push(&[0.1]);
        assert!(err.is_err());
    }

    #[test]
    fn to_audio_input() {
        let mut buf = PcmBuffer::dictation();
        buf.push(&vec![0.0; 1600]).unwrap();
        let audio = buf.to_audio_input().unwrap();
        assert_eq!(audio.sample_rate, 16_000);
        assert_eq!(audio.len(), 1600);
        assert!(!buf.is_empty());
        let taken = buf.take_audio_input().unwrap();
        assert_eq!(taken.len(), 1600);
        assert!(buf.is_empty());
    }
}
