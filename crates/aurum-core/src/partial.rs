//! Host-driven partial transcription session (JOE-1605).
//!
//! Aurum does not own microphone devices. Hosts push PCM, and this module
//! decides when to decode, how to stabilize prefixes, and how to discard
//! superseded results.

use crate::cancel::CancelFlag;
use crate::pcm::PcmBuffer;
use crate::window::{PartialClock, PartialWindowPolicy};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

/// Why a partial or final emission was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizationReason {
    /// Periodic partial decode on the rolling window.
    Interval,
    /// Energy/VAD gate passed after enough audio.
    EnergyGate,
    /// Host requested a definitive final transcript.
    Final,
    /// Session cancelled.
    Cancelled,
}

/// One partial emission for the host UI.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialUpdate {
    /// Monotonic revision (later supersedes earlier).
    pub revision: u64,
    /// Prefix that will not change under the documented algorithm.
    pub stable_text: String,
    /// Tail that may still change on subsequent partials.
    pub unstable_text: String,
    /// Combined display text (`stable` + `unstable`).
    pub display_text: String,
    /// Sample count of the window that produced this text.
    pub window_samples: usize,
    /// Wall latency for this decode (host fills when known).
    pub latency_ms: Option<u64>,
    pub reason: FinalizationReason,
    /// True when this is the definitive session result.
    pub is_final: bool,
}

/// Configuration for partial session v2.
#[derive(Debug, Clone)]
pub struct PartialSessionConfig {
    pub window: PartialWindowPolicy,
    /// Characters of the previous display kept as stable when the new decode
    /// shares that prefix (simple longest-common-prefix stabilization).
    pub stabilize_min_chars: usize,
    /// Maximum in-flight partial decodes; further requests are dropped.
    pub max_inflight: usize,
}

impl Default for PartialSessionConfig {
    fn default() -> Self {
        Self {
            window: PartialWindowPolicy::dictation(),
            stabilize_min_chars: 8,
            max_inflight: 1,
        }
    }
}

/// Host-driven partial STT session state.
///
/// Typical loop:
/// 1. `push` mic chunks
/// 2. if `should_decode_partial`, host runs STT on `partial_window_samples()`
/// 3. call `on_decode_result` with the transcript text
/// 4. on release, `finalize` with a definitive full-window decode
pub struct PartialSession {
    config: PartialSessionConfig,
    clock: PartialClock,
    buffer: PcmBuffer,
    stable_text: String,
    last_display: String,
    last_revision: u64,
    inflight: usize,
    /// Cancel token for the currently queued/running partial (if any).
    active_cancel: Option<CancelFlag>,
    closed: bool,
}

impl PartialSession {
    pub fn new(config: PartialSessionConfig) -> Self {
        let max_secs = config
            .window
            .window_secs()
            .max(config.window.min_partial_secs())
            * 2.0;
        let clock = PartialClock::new(config.window);
        Self {
            config,
            clock,
            buffer: PcmBuffer::with_max_secs(max_secs.max(1.0)),
            stable_text: String::new(),
            last_display: String::new(),
            last_revision: 0,
            inflight: 0,
            active_cancel: None,
            closed: false,
        }
    }

    /// Dictation-oriented defaults.
    pub fn dictation() -> Self {
        Self::new(PartialSessionConfig::default())
    }

    pub fn config(&self) -> &PartialSessionConfig {
        &self.config
    }

    pub fn buffer(&self) -> &PcmBuffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut PcmBuffer {
        &mut self.buffer
    }

    pub fn stable_text(&self) -> &str {
        &self.stable_text
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Push mono 16 kHz samples into the ring buffer.
    pub fn push(&mut self, chunk: &[f32]) -> crate::error::Result<()> {
        if self.closed {
            return Err(crate::error::UserError::Other {
                message: "partial session is closed".into(),
            }
            .into());
        }
        self.buffer.push(chunk)
    }

    /// Whether the host should start a partial decode now.
    pub fn should_decode_partial(&self) -> bool {
        if self.closed || self.inflight >= self.config.max_inflight {
            return false;
        }
        let samples = self.buffer.samples();
        self.clock.ready(samples.as_slice())
    }

    /// Contiguous PCM for the current partial window (host passes to STT).
    pub fn partial_window_samples(&self) -> ContiguousOwned {
        let samples = self.buffer.samples();
        let window = self.config.window.slice_for_partial(samples.as_slice());
        ContiguousOwned(window.to_vec())
    }

    /// Begin a partial: marks the clock, increments inflight, returns cancel token.
    ///
    /// Supersedes any previous inflight cancel token (cooperative cancel).
    pub fn begin_partial(&mut self) -> Option<CancelFlag> {
        if !self.should_decode_partial() {
            return None;
        }
        if let Some(prev) = self.active_cancel.take() {
            prev.cancel();
        }
        let flag = CancelFlag::new();
        self.active_cancel = Some(flag.clone());
        self.inflight = self
            .inflight
            .saturating_add(1)
            .min(self.config.max_inflight);
        self.clock.mark();
        Some(flag)
    }

    /// Host delivers a partial decode result (or error → call [`Self::end_partial`]).
    pub fn on_decode_result(&mut self, raw_text: &str, latency_ms: Option<u64>) -> PartialUpdate {
        self.inflight = self.inflight.saturating_sub(1);
        self.active_cancel = None;

        let display = normalize_partial_text(raw_text);
        let (stable, unstable) = stabilize_prefix(
            &self.stable_text,
            &self.last_display,
            &display,
            self.config.stabilize_min_chars,
        );
        self.stable_text = stable.clone();
        self.last_display = display.clone();

        let revision = NEXT_REVISION.fetch_add(1, Ordering::Relaxed);
        self.last_revision = revision;
        let window_samples = self
            .config
            .window
            .slice_for_partial(self.buffer.samples().as_slice())
            .len();

        PartialUpdate {
            revision,
            stable_text: stable,
            unstable_text: unstable,
            display_text: display,
            window_samples,
            latency_ms,
            reason: FinalizationReason::Interval,
            is_final: false,
        }
    }

    /// Mark a partial as finished without a text update (cancel/error).
    pub fn end_partial(&mut self) {
        self.inflight = self.inflight.saturating_sub(1);
        self.active_cancel = None;
    }

    /// Final decode path: host supplies definitive transcript for the full buffer.
    pub fn finalize(&mut self, final_text: &str, latency_ms: Option<u64>) -> PartialUpdate {
        if let Some(prev) = self.active_cancel.take() {
            prev.cancel();
        }
        self.inflight = 0;
        self.closed = true;
        let display = normalize_partial_text(final_text);
        self.stable_text = display.clone();
        self.last_display = display.clone();
        let revision = NEXT_REVISION.fetch_add(1, Ordering::Relaxed);
        self.last_revision = revision;
        PartialUpdate {
            revision,
            stable_text: display.clone(),
            unstable_text: String::new(),
            display_text: display,
            window_samples: self.buffer.len(),
            latency_ms,
            reason: FinalizationReason::Final,
            is_final: true,
        }
    }

    /// Cancel the session and any in-flight partial.
    pub fn cancel(&mut self) -> PartialUpdate {
        if let Some(prev) = self.active_cancel.take() {
            prev.cancel();
        }
        self.inflight = 0;
        self.closed = true;
        let revision = NEXT_REVISION.fetch_add(1, Ordering::Relaxed);
        PartialUpdate {
            revision,
            stable_text: self.stable_text.clone(),
            unstable_text: String::new(),
            display_text: self.stable_text.clone(),
            window_samples: self.buffer.len(),
            latency_ms: None,
            reason: FinalizationReason::Cancelled,
            is_final: true,
        }
    }
}

/// Owned contiguous samples for passing into STT without lifetime coupling.
#[derive(Debug, Clone)]
pub struct ContiguousOwned(pub Vec<f32>);

impl ContiguousOwned {
    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }
}

impl AsRef<[f32]> for ContiguousOwned {
    fn as_ref(&self) -> &[f32] {
        &self.0
    }
}

fn normalize_partial_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Longest common prefix stabilization.
///
/// Once a prefix is in `prior_stable`, it is never shortened. The new stable
/// region is the longest common prefix of `prior_display` and `new_display`
/// that is at least `min_chars`, merged with `prior_stable`.
fn stabilize_prefix(
    prior_stable: &str,
    prior_display: &str,
    new_display: &str,
    min_chars: usize,
) -> (String, String) {
    if new_display.is_empty() {
        return (prior_stable.to_string(), String::new());
    }
    // Never retract prior stable.
    if let Some(_tail) = new_display.strip_prefix(prior_stable) {
        // Grow stable from LCP of previous full display and new.
        let lcp = longest_common_prefix(prior_display, new_display);
        let grow_to = if lcp.len() >= min_chars && lcp.len() > prior_stable.len() {
            lcp
        } else {
            prior_stable
        };
        let stable = if new_display.starts_with(grow_to) && grow_to.len() >= prior_stable.len() {
            grow_to.to_string()
        } else {
            prior_stable.to_string()
        };
        let unstable = new_display.strip_prefix(&stable).unwrap_or("").to_string();
        return (stable, unstable);
    }
    if prior_stable.is_empty() {
        let lcp = longest_common_prefix(prior_display, new_display);
        if lcp.len() >= min_chars {
            (
                lcp.to_string(),
                new_display.strip_prefix(lcp).unwrap_or("").to_string(),
            )
        } else {
            (String::new(), new_display.to_string())
        }
    } else {
        // Do not rewrite stable after commitment.
        (prior_stable.to_string(), String::new())
    }
}

fn longest_common_prefix<'a>(a: &'a str, b: &'a str) -> &'a str {
    let mut end = 0;
    let mut ait = a.char_indices();
    let mut bit = b.chars();
    loop {
        match (ait.next(), bit.next()) {
            (Some((i, ca)), Some(cb)) if ca == cb => {
                end = i + ca.len_utf8();
            }
            _ => break,
        }
    }
    &a[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_never_retracts() {
        let mut s = PartialSession::dictation();
        // Seed with enough samples for ready checks later.
        s.push(&vec![0.1; 16_000]).unwrap();
        let u1 = s.on_decode_result("hello world today", None);
        assert!(u1.display_text.contains("hello"));
        let u2 = s.on_decode_result("hello world tomorrow", None);
        assert!(u2.stable_text.starts_with(&u1.stable_text) || u2.stable_text == "hello world ");
        // Prior stable must remain a prefix of stable after growth, or equal.
        assert!(u2.stable_text.starts_with("hello") || s.stable_text().starts_with("hello"));
    }

    #[test]
    fn inflight_cap_blocks() {
        let mut s = PartialSession::new(PartialSessionConfig {
            max_inflight: 1,
            window: PartialWindowPolicy {
                min_partial_samples: 10,
                window_samples: 1000,
                interval_nanos: 0,
                min_rms_bits: 0,
            },
            ..Default::default()
        });
        s.push(&vec![0.2; 100]).unwrap();
        assert!(s.should_decode_partial());
        let _c = s.begin_partial().unwrap();
        assert!(!s.should_decode_partial());
        s.end_partial();
        assert!(s.should_decode_partial());
    }

    #[test]
    fn cancel_supersedes() {
        let mut s = PartialSession::new(PartialSessionConfig {
            max_inflight: 1,
            window: PartialWindowPolicy {
                min_partial_samples: 1,
                window_samples: 100,
                interval_nanos: 0,
                min_rms_bits: 0,
            },
            ..Default::default()
        });
        s.push(&[0.2; 50]).unwrap();
        let _c1 = s.begin_partial().unwrap();
        s.end_partial();
        s.push(&[0.2; 10]).unwrap();
        let _c2 = s.begin_partial();
        let fin = s.cancel();
        assert!(fin.is_final);
        assert_eq!(fin.reason, FinalizationReason::Cancelled);
    }

    #[test]
    fn finalize_commits_all_stable() {
        let mut s = PartialSession::dictation();
        s.push(&vec![0.1; 1600]).unwrap();
        let u = s.finalize("final transcript here", Some(12));
        assert!(u.is_final);
        assert_eq!(u.stable_text, "final transcript here");
        assert!(u.unstable_text.is_empty());
        assert!(s.is_closed());
    }

    #[test]
    fn lcp_unicode() {
        assert_eq!(longest_common_prefix("café", "cafx"), "caf");
        assert_eq!(longest_common_prefix("abc", "xyz"), "");
    }
}
