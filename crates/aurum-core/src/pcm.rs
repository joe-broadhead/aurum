//! In-memory PCM buffer for mic / streaming hosts (no files, no ffmpeg).
//!
//! Fixed-capacity ring buffer with O(1) rolling (JOE-1601). Invalid floating-point
//! samples are rejected before they reach DSP / native code.
//!
//! Thread-safety: `PcmBuffer` is **not** synchronized. Hosts that share a buffer
//! across threads must provide their own mutex.

use crate::audio::{AudioInput, WHISPER_SAMPLE_RATE};
use crate::error::{Result, UserError};
use std::sync::Arc;

/// Default max buffer for interactive dictation (~60 s @ 16 kHz), matching
/// common hold-to-talk products. Longer content should use file transcription.
pub const DEFAULT_DICTATION_MAX_SECS: f64 = 60.0;

/// Rolling / accumulating mono PCM buffer at [`WHISPER_SAMPLE_RATE`].
///
/// Storage is a fixed-capacity ring. Capacity is never exceeded, including when a
/// single push is larger than capacity (only the newest tail is retained).
#[derive(Debug, Clone)]
pub struct PcmBuffer {
    /// Fixed storage of length `max_samples`.
    storage: Vec<f32>,
    /// Index of the oldest sample when `len == max_samples` (and always the
    /// logical start of the ring contents).
    head: usize,
    /// Number of valid samples currently stored (≤ max_samples).
    len: usize,
    max_samples: usize,
    /// When true, overflow drops oldest samples (rolling window).
    /// When false, [`push`](Self::push) errors on overflow.
    rolling: bool,
    /// Count of out-of-range samples clamped into [-1, 1] this session.
    clamped_count: u64,
}

impl PcmBuffer {
    /// Create a buffer capped at `max_secs` of audio (rolling by default).
    pub fn with_max_secs(max_secs: f64) -> Self {
        let max_samples = (max_secs.max(0.0) * f64::from(WHISPER_SAMPLE_RATE)).round() as usize;
        let max_samples = max_samples.max(1);
        Self {
            storage: vec![0.0; max_samples],
            head: 0,
            len: 0,
            max_samples,
            rolling: true,
            clamped_count: 0,
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
        self.head = 0;
        self.len = 0;
        // Leave storage contents undefined for performance; `len` is the authority.
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn max_samples(&self) -> usize {
        self.max_samples
    }

    pub fn is_rolling(&self) -> bool {
        self.rolling
    }

    pub fn duration_secs(&self) -> f64 {
        self.len as f64 / f64::from(WHISPER_SAMPLE_RATE)
    }

    /// How many samples were amplitude-clamped into [-1, 1] since construction.
    pub fn clamped_count(&self) -> u64 {
        self.clamped_count
    }

    /// Contiguous view of logical samples.
    ///
    /// When the ring is not wrapped this is zero-copy. When wrapped, samples are
    /// linearized into a temporary `Vec` (only on this call).
    pub fn samples(&self) -> ContiguousSamples<'_> {
        if self.len == 0 {
            return ContiguousSamples::Borrowed(&[]);
        }
        let cap = self.max_samples;
        let first = self.head % cap;
        if first + self.len <= cap {
            ContiguousSamples::Borrowed(&self.storage[first..first + self.len])
        } else {
            let mut out = Vec::with_capacity(self.len);
            let n1 = cap - first;
            out.extend_from_slice(&self.storage[first..]);
            out.extend_from_slice(&self.storage[..self.len - n1]);
            ContiguousSamples::Owned(out)
        }
    }

    /// Copy logical samples into `dst` (reused by hosts to avoid allocation).
    pub fn copy_samples_into(&self, dst: &mut Vec<f32>) {
        dst.clear();
        if self.len == 0 {
            return;
        }
        dst.reserve(self.len);
        let cap = self.max_samples;
        let first = self.head % cap;
        if first + self.len <= cap {
            dst.extend_from_slice(&self.storage[first..first + self.len]);
        } else {
            let n1 = cap - first;
            dst.extend_from_slice(&self.storage[first..]);
            dst.extend_from_slice(&self.storage[..self.len - n1]);
        }
    }

    /// Append mono f32 samples already at 16 kHz.
    ///
    /// Rejects NaN/Inf. Samples outside [-1, 1] are clamped and counted.
    /// Oversized chunks in rolling mode keep only the newest `max_samples` without
    /// ever allocating beyond capacity.
    pub fn push(&mut self, chunk: &[f32]) -> Result<()> {
        if chunk.is_empty() {
            return Ok(());
        }
        validate_finite(chunk)?;

        if !self.rolling {
            if self.len.saturating_add(chunk.len()) > self.max_samples {
                return Err(UserError::AudioTooLong {
                    duration_secs: (self.len + chunk.len()) as f64 / f64::from(WHISPER_SAMPLE_RATE),
                    max_secs: self.max_samples as f64 / f64::from(WHISPER_SAMPLE_RATE),
                }
                .into());
            }
            for &s in chunk {
                self.push_one(s);
            }
            return Ok(());
        }

        // Rolling: if the chunk alone exceeds capacity, keep only its newest tail.
        let src = if chunk.len() > self.max_samples {
            &chunk[chunk.len() - self.max_samples..]
        } else {
            chunk
        };
        for &s in src {
            self.push_one(s);
        }
        Ok(())
    }

    fn push_one(&mut self, sample: f32) {
        let s = if sample.is_finite() {
            if sample > 1.0 {
                self.clamped_count = self.clamped_count.saturating_add(1);
                1.0
            } else if sample < -1.0 {
                self.clamped_count = self.clamped_count.saturating_add(1);
                -1.0
            } else {
                sample
            }
        } else {
            // validate_finite already rejected NaN/Inf on the public path.
            0.0
        };

        let cap = self.max_samples;
        if self.len < cap {
            let idx = (self.head + self.len) % cap;
            self.storage[idx] = s;
            self.len += 1;
        } else {
            // Overwrite oldest.
            self.storage[self.head] = s;
            self.head = (self.head + 1) % cap;
        }
    }

    /// RMS energy using `f64` accumulation (numerically stable for long windows).
    pub fn rms(&self) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        let mut sum = 0.0f64;
        let cap = self.max_samples;
        for i in 0..self.len {
            let s = self.storage[(self.head + i) % cap] as f64;
            sum += s * s;
        }
        (sum / self.len as f64).sqrt() as f32
    }

    /// Snapshot as [`AudioInput`] without clearing the buffer.
    pub fn to_audio_input(&self) -> Result<AudioInput> {
        if self.len == 0 {
            return Err(UserError::InvalidAudio {
                reason: "PCM buffer is empty".into(),
            }
            .into());
        }
        let mut v = Vec::with_capacity(self.len);
        self.copy_samples_into(&mut v);
        let samples: Arc<[f32]> = v.into();
        AudioInput::from_pcm(samples, WHISPER_SAMPLE_RATE)
    }

    /// Take ownership of samples and clear the buffer.
    pub fn take_audio_input(&mut self) -> Result<AudioInput> {
        if self.len == 0 {
            return Err(UserError::InvalidAudio {
                reason: "PCM buffer is empty".into(),
            }
            .into());
        }
        let mut v = Vec::with_capacity(self.len);
        self.copy_samples_into(&mut v);
        self.clear();
        let samples: Arc<[f32]> = v.into();
        AudioInput::from_pcm(samples, WHISPER_SAMPLE_RATE)
    }
}

/// Contiguous sample view that may own a linearized copy when the ring is wrapped.
#[derive(Debug)]
pub enum ContiguousSamples<'a> {
    Borrowed(&'a [f32]),
    Owned(Vec<f32>),
}

impl ContiguousSamples<'_> {
    pub fn as_slice(&self) -> &[f32] {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(v) => v.as_slice(),
        }
    }
}

impl std::ops::Deref for ContiguousSamples<'_> {
    type Target = [f32];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

fn validate_finite(chunk: &[f32]) -> Result<()> {
    for (i, s) in chunk.iter().enumerate() {
        if !s.is_finite() {
            return Err(UserError::InvalidAudio {
                reason: format!("PCM contains non-finite sample at index {i} ({s})"),
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
    fn rolling_drops_oldest() {
        let mut buf = PcmBuffer::with_max_secs(0.01); // 160 samples
        assert_eq!(buf.max_samples(), 160);
        buf.push(&vec![0.1; 100]).unwrap();
        buf.push(&vec![0.2; 100]).unwrap();
        assert_eq!(buf.len(), 160);
        let s = buf.samples();
        assert_eq!(s[0], 0.1);
        assert_eq!(s[59], 0.1);
        assert_eq!(s[60], 0.2);
    }

    #[test]
    fn oversized_chunk_keeps_tail_only() {
        let mut buf = PcmBuffer::with_max_secs(0.01); // 160
                                                      // Encode index in a sub-unit range so finite/clamp checks pass.
        let big: Vec<f32> = (0..500).map(|i| (i as f32) * 0.001).collect();
        buf.push(&big).unwrap();
        assert_eq!(buf.len(), 160);
        let s = buf.samples();
        // Newest 160 values: indices 340..499
        assert!((s[0] - 0.340).abs() < 1e-6);
        assert!((s[159] - 0.499).abs() < 1e-6);
        assert_eq!(buf.storage.len(), 160);
    }

    #[test]
    fn bounded_errors() {
        let mut buf = PcmBuffer::bounded(0.01);
        buf.push(&vec![0.1; 160]).unwrap();
        let err = buf.push(&[0.1]);
        assert!(err.is_err());
    }

    #[test]
    fn rejects_nan() {
        let mut buf = PcmBuffer::dictation();
        let err = buf.push(&[0.0, f32::NAN, 0.1]);
        assert!(err.is_err());
        assert!(buf.is_empty());
    }

    #[test]
    fn clamps_amplitude() {
        let mut buf = PcmBuffer::dictation();
        buf.push(&[2.0, -3.0, 0.5]).unwrap();
        assert_eq!(buf.clamped_count(), 2);
        let s = buf.samples();
        assert_eq!(s[0], 1.0);
        assert_eq!(s[1], -1.0);
        assert_eq!(s[2], 0.5);
    }

    #[test]
    fn wraparound_contiguous() {
        let mut buf = PcmBuffer::with_max_secs(0.001); // 16 samples
        buf.push(&[0.1; 12]).unwrap();
        buf.push(&[0.2; 12]).unwrap();
        assert_eq!(buf.len(), 16);
        let s = buf.samples();
        // 4 remaining 0.1 + 12 of 0.2
        assert_eq!(&s[..4], &[0.1; 4]);
        assert_eq!(&s[4..], &[0.2; 12]);
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

    #[test]
    fn rms_f64() {
        let mut buf = PcmBuffer::dictation();
        buf.push(&[0.5; 100]).unwrap();
        let r = buf.rms();
        assert!((r - 0.5).abs() < 1e-5);
    }
}
