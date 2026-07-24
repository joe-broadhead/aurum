//! Rolling-window helpers for host-driven “partial-like” decode loops.
//!
//! Aurum does not run a background streaming decoder. Hosts (e.g. dictation apps)
//! can push PCM into a [`crate::pcm::PcmBuffer`], then use these policies to decide
//! when to call [`crate::LocalWhisperProvider::transcribe_pcm`] on a slice.

use crate::audio::WHISPER_SAMPLE_RATE;

/// Policy aligned with common hold-to-talk UX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartialWindowPolicy {
    /// Minimum samples before a partial is worth running (~1 s default).
    pub min_partial_samples: usize,
    /// Max samples included in a partial slice (~15 s default).
    pub window_samples: usize,
    /// Suggested spacing between partial attempts (nanoseconds).
    pub interval_nanos: u64,
    /// RMS below this → skip partial (near-silence).
    pub min_rms_bits: u32, // store as fixed-point * 1e6 for Eq; use helpers
}

impl PartialWindowPolicy {
    /// Defaults suited to progressive hold-to-talk decode.
    pub fn dictation() -> Self {
        Self {
            min_partial_samples: WHISPER_SAMPLE_RATE as usize, // 1 s
            window_samples: WHISPER_SAMPLE_RATE as usize * 15, // 15 s
            interval_nanos: 1_200_000_000,                     // 1.2 s
            min_rms_bits: 500,                                 // 0.0005
        }
    }

    pub fn min_rms(&self) -> f32 {
        self.min_rms_bits as f32 / 1_000_000.0
    }

    pub fn with_min_rms(mut self, rms: f32) -> Self {
        self.min_rms_bits = (rms.clamp(0.0, 1.0) * 1_000_000.0).round() as u32;
        self
    }

    pub fn min_partial_secs(&self) -> f64 {
        self.min_partial_samples as f64 / f64::from(WHISPER_SAMPLE_RATE)
    }

    pub fn window_secs(&self) -> f64 {
        self.window_samples as f64 / f64::from(WHISPER_SAMPLE_RATE)
    }

    /// Enough audio accumulated for a partial attempt?
    pub fn can_run_partial(&self, sample_count: usize) -> bool {
        sample_count >= self.min_partial_samples
    }

    /// Most recent `window_samples` (or all if shorter).
    pub fn slice_for_partial<'a>(&self, samples: &'a [f32]) -> &'a [f32] {
        if samples.len() > self.window_samples {
            &samples[samples.len() - self.window_samples..]
        } else {
            samples
        }
    }

    /// Energy gate: skip near-silent buffers.
    pub fn passes_energy_gate(&self, samples: &[f32]) -> bool {
        if samples.is_empty() {
            return false;
        }
        let sum: f32 = samples.iter().map(|s| s * s).sum();
        let rms = (sum / samples.len() as f32).sqrt();
        rms >= self.min_rms()
    }

    /// Combined: enough samples + energy.
    pub fn should_run_partial(&self, samples: &[f32]) -> bool {
        self.can_run_partial(samples.len()) && self.passes_energy_gate(samples)
    }
}

/// Tracks wall-clock spacing between partial attempts.
#[derive(Debug, Clone)]
pub struct PartialClock {
    policy: PartialWindowPolicy,
    last_partial_at: Option<std::time::Instant>,
}

impl PartialClock {
    pub fn new(policy: PartialWindowPolicy) -> Self {
        Self {
            policy,
            last_partial_at: None,
        }
    }

    pub fn policy(&self) -> &PartialWindowPolicy {
        &self.policy
    }

    /// True if enough time has elapsed since the last partial (or never ran).
    pub fn interval_elapsed(&self) -> bool {
        match self.last_partial_at {
            None => true,
            Some(t) => t.elapsed().as_nanos() as u64 >= self.policy.interval_nanos,
        }
    }

    /// Mark that a partial was started/completed now.
    pub fn mark(&mut self) {
        self.last_partial_at = Some(std::time::Instant::now());
    }

    /// Ready for another partial given full buffer samples.
    /// Energy is evaluated on the same rolling window that would be decoded.
    pub fn ready(&self, samples: &[f32]) -> bool {
        if !self.interval_elapsed() || !self.policy.can_run_partial(samples.len()) {
            return false;
        }
        let window = self.policy.slice_for_partial(samples);
        self.policy.passes_energy_gate(window)
    }

    /// Slice + mark if ready; returns `None` if not ready.
    pub fn take_partial_slice<'a>(&mut self, samples: &'a [f32]) -> Option<&'a [f32]> {
        if !self.ready(samples) {
            return None;
        }
        self.mark();
        Some(self.policy.slice_for_partial(samples))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_takes_tail() {
        let p = PartialWindowPolicy {
            window_samples: 4,
            min_partial_samples: 2,
            interval_nanos: 0,
            min_rms_bits: 0,
        };
        let s = [1., 2., 3., 4., 5., 6.];
        assert_eq!(p.slice_for_partial(&s), &[3., 4., 5., 6.]);
    }

    #[test]
    fn energy_gate() {
        let p = PartialWindowPolicy::dictation().with_min_rms(0.01);
        assert!(!p.passes_energy_gate(&[0.0; 1600]));
        assert!(p.passes_energy_gate(&[0.5; 1600]));
    }

    #[test]
    fn clock_interval() {
        let mut c = PartialClock::new(PartialWindowPolicy {
            min_partial_samples: 1,
            window_samples: 100,
            interval_nanos: 10_000_000_000, // 10s
            min_rms_bits: 0,
        });
        let s = [0.1; 10];
        assert!(c.ready(&s));
        c.mark();
        assert!(!c.ready(&s));
    }
}
