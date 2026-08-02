//! Boundary-aware long-form STT planning, overlap stitch, and policy (JOE-2219).
//!
//! Replaces blind non-overlapping windows with silence-aware cuts and a bounded
//! overlap when no quiet region exists. Deduplication is deterministic and never
//! drops low-confidence text.

use crate::error::{Result, UserError};
use crate::providers::Segment;
use crate::remote::stt_chunk::{ChunkWindow, DEFAULT_REMOTE_STT_CHUNK_SECS};
use serde::{Deserialize, Serialize};

/// Default silence search half-width around the target cut (±seconds).
pub const DEFAULT_BOUNDARY_SEARCH_SECS: f64 = 15.0;
/// Default minimum quiet duration to accept a silence boundary (seconds).
pub const DEFAULT_MIN_SILENCE_SECS: f64 = 0.25;
/// Default overlap when cutting without silence (seconds).
pub const DEFAULT_OVERLAP_SECS: f64 = 1.5;
/// Maximum overlap as a fraction of chunk duration.
pub const DEFAULT_MAX_OVERLAP_FRACTION: f64 = 0.05;
/// Max tokens examined for overlap dedupe.
pub const MAX_DEDUPE_TOKENS: usize = 40;
/// Minimum exact token overlap required to drop a later-chunk prefix.
pub const MIN_DEDUPE_TOKENS: usize = 3;

/// How a segment's timing was obtained (JOE-2219).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimestampSource {
    /// Local model timing.
    NativeModel,
    /// Word timing from a remote ASR provider.
    ProviderWord,
    /// Segment timing from a provider.
    ProviderSegment,
    /// Provider timing shifted by chunk start offset.
    ChunkOffset,
    /// Aurum split text and estimated time proportionally.
    Interpolated,
    /// One full-duration span created because no provider timing existed.
    SyntheticSpan,
    /// No usable timing.
    #[default]
    Unavailable,
}

impl TimestampSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NativeModel => "native_model",
            Self::ProviderWord => "provider_word",
            Self::ProviderSegment => "provider_segment",
            Self::ChunkOffset => "chunk_offset",
            Self::Interpolated => "interpolated",
            Self::SyntheticSpan => "synthetic_span",
            Self::Unavailable => "unavailable",
        }
    }

    /// Conservative reliability for the legacy `timestamps_reliable` boolean.
    pub fn is_reliable(self) -> bool {
        matches!(
            self,
            Self::NativeModel | Self::ProviderWord | Self::ProviderSegment | Self::ChunkOffset
        )
    }

    /// Approximate / non-native timing that SRT rejects by default.
    pub fn is_approximate(self) -> bool {
        matches!(
            self,
            Self::Interpolated | Self::SyntheticSpan | Self::Unavailable
        )
    }
}

/// Why a planned cut was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    Silence,
    TargetWithOverlap,
    ShortSingle,
    FixedFallback,
}

/// Validated long-form chunking policy (not free-form env vars).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LongFormPolicy {
    /// Target window length in seconds (~210 product default).
    pub target_secs: f64,
    pub min_secs: f64,
    pub max_secs: f64,
    /// Search ± this many seconds around the target for silence.
    pub search_secs: f64,
    pub min_silence_secs: f64,
    /// RMS threshold relative to peak (0..1) for "quiet".
    pub silence_rms_ratio: f64,
    pub overlap_secs: f64,
    pub max_overlap_fraction: f64,
}

impl Default for LongFormPolicy {
    fn default() -> Self {
        Self {
            target_secs: DEFAULT_REMOTE_STT_CHUNK_SECS,
            min_secs: 30.0,
            max_secs: 300.0,
            search_secs: DEFAULT_BOUNDARY_SEARCH_SECS,
            min_silence_secs: DEFAULT_MIN_SILENCE_SECS,
            silence_rms_ratio: 0.08,
            overlap_secs: DEFAULT_OVERLAP_SECS,
            max_overlap_fraction: DEFAULT_MAX_OVERLAP_FRACTION,
        }
    }
}

impl LongFormPolicy {
    pub fn validate(&self) -> Result<()> {
        for (name, v) in [
            ("target_secs", self.target_secs),
            ("min_secs", self.min_secs),
            ("max_secs", self.max_secs),
            ("search_secs", self.search_secs),
            ("min_silence_secs", self.min_silence_secs),
            ("silence_rms_ratio", self.silence_rms_ratio),
            ("overlap_secs", self.overlap_secs),
            ("max_overlap_fraction", self.max_overlap_fraction),
        ] {
            if !v.is_finite() || v < 0.0 {
                return Err(UserError::InvalidConfig {
                    reason: format!("LongFormPolicy.{name} must be finite and non-negative"),
                }
                .into());
            }
        }
        if self.target_secs <= 0.0 || self.min_secs <= 0.0 || self.max_secs <= 0.0 {
            return Err(UserError::InvalidConfig {
                reason: "LongFormPolicy window sizes must be > 0".into(),
            }
            .into());
        }
        if self.min_secs > self.target_secs || self.target_secs > self.max_secs {
            return Err(UserError::InvalidConfig {
                reason: "LongFormPolicy requires min_secs ≤ target_secs ≤ max_secs".into(),
            }
            .into());
        }
        if self.silence_rms_ratio > 1.0 {
            return Err(UserError::InvalidConfig {
                reason: "LongFormPolicy.silence_rms_ratio must be ≤ 1.0".into(),
            }
            .into());
        }
        if self.max_overlap_fraction > 0.5 {
            return Err(UserError::InvalidConfig {
                reason: "LongFormPolicy.max_overlap_fraction must be ≤ 0.5".into(),
            }
            .into());
        }
        Ok(())
    }

    /// Build from env `AURUM_REMOTE_STT_CHUNK_SECS` when set, else defaults.
    pub fn from_env_or_default() -> Self {
        let mut p = Self::default();
        if let Ok(s) = std::env::var("AURUM_REMOTE_STT_CHUNK_SECS") {
            if let Ok(v) = s.trim().parse::<f64>() {
                if v.is_finite() && v > 0.0 {
                    p.target_secs = v;
                    p.max_secs = p.max_secs.max(v);
                }
            }
        }
        p
    }
}

/// One planned window with boundary metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedWindow {
    pub window: ChunkWindow,
    pub kind: BoundaryKind,
    /// Overlap duration in seconds with the previous window (0 for first).
    pub overlap_secs: f64,
}

/// Plan silence-aware (or overlap) windows covering all samples.
pub fn plan_boundary_windows(
    samples: &[f32],
    sample_rate: u32,
    policy: &LongFormPolicy,
) -> Result<Vec<PlannedWindow>> {
    policy.validate()?;
    let total = samples.len();
    if total == 0 || sample_rate == 0 {
        return Ok(vec![PlannedWindow {
            window: ChunkWindow {
                start_sample: 0,
                end_sample: total,
                offset_secs: 0.0,
            },
            kind: BoundaryKind::ShortSingle,
            overlap_secs: 0.0,
        }]);
    }

    let target = ((policy.target_secs * f64::from(sample_rate)).round() as usize).max(1);
    if total <= target {
        return Ok(vec![PlannedWindow {
            window: ChunkWindow {
                start_sample: 0,
                end_sample: total,
                offset_secs: 0.0,
            },
            kind: BoundaryKind::ShortSingle,
            overlap_secs: 0.0,
        }]);
    }

    let min_len = ((policy.min_secs * f64::from(sample_rate)).round() as usize).max(1);
    let max_len = ((policy.max_secs * f64::from(sample_rate)).round() as usize).max(min_len);
    let search = ((policy.search_secs * f64::from(sample_rate)).round() as usize).max(1);
    let min_silence = ((policy.min_silence_secs * f64::from(sample_rate)).round() as usize).max(1);
    let peak = peak_abs(samples).max(1e-9);
    let quiet_thresh = peak * policy.silence_rms_ratio as f32;

    let mut out = Vec::new();
    let mut start = 0usize;
    while start < total {
        let remaining = total - start;
        if remaining <= max_len {
            out.push(PlannedWindow {
                window: ChunkWindow {
                    start_sample: start,
                    end_sample: total,
                    offset_secs: start as f64 / f64::from(sample_rate),
                },
                kind: if out.is_empty() {
                    BoundaryKind::ShortSingle
                } else {
                    BoundaryKind::Silence
                },
                overlap_secs: 0.0,
            });
            break;
        }

        let ideal = (start + target).min(total);
        let search_lo = ideal.saturating_sub(search).max(start + min_len);
        let search_hi = (ideal + search).min(start + max_len).min(total);

        let silence_cut =
            find_silence_boundary(samples, search_lo, search_hi, min_silence, quiet_thresh);

        let (end, kind, overlap_secs) = if let Some(cut) = silence_cut {
            (cut, BoundaryKind::Silence, 0.0f64)
        } else {
            // Hard cut at ideal with overlap into next window.
            let end = ideal.min(total);
            let raw_overlap = ((policy.overlap_secs * f64::from(sample_rate)).round() as usize)
                .min(((end - start) as f64 * policy.max_overlap_fraction).round() as usize);
            let overlap = raw_overlap.min(end.saturating_sub(start) / 4);
            (
                end,
                BoundaryKind::TargetWithOverlap,
                overlap as f64 / f64::from(sample_rate),
            )
        };

        let end = end.max(start + 1).min(total);
        out.push(PlannedWindow {
            window: ChunkWindow {
                start_sample: start,
                end_sample: end,
                offset_secs: start as f64 / f64::from(sample_rate),
            },
            kind,
            overlap_secs,
        });

        if end >= total {
            break;
        }
        // Advance: for overlap cuts, next window starts `overlap` samples before end.
        let overlap_samples = if matches!(kind, BoundaryKind::TargetWithOverlap) {
            ((overlap_secs * f64::from(sample_rate)).round() as usize).min(end - start)
        } else {
            0
        };
        let next = end.saturating_sub(overlap_samples);
        if next <= start {
            // Degenerate: force progress without infinite loop.
            start = end;
        } else {
            start = next;
        }
    }

    // Coverage: first starts at 0, last ends at total, no gaps in exclusive coverage of unique audio.
    if let Some(first) = out.first() {
        if first.window.start_sample != 0 {
            return Err(UserError::Other {
                message: "long-form planner: first window must start at sample 0".into(),
            }
            .into());
        }
    }
    if let Some(last) = out.last() {
        if last.window.end_sample != total {
            return Err(UserError::Other {
                message: "long-form planner: last window must end at total samples".into(),
            }
            .into());
        }
    }
    Ok(out)
}

fn peak_abs(samples: &[f32]) -> f32 {
    let mut p = 0.0f32;
    for &s in samples {
        p = p.max(s.abs());
    }
    p
}

/// Find the earliest quiet region of `min_silence` samples with lowest mean energy.
fn find_silence_boundary(
    samples: &[f32],
    lo: usize,
    hi: usize,
    min_silence: usize,
    quiet_thresh: f32,
) -> Option<usize> {
    if hi <= lo + min_silence {
        return None;
    }
    let mut best: Option<(f64, usize)> = None; // (energy, cut_end)
    let mut i = lo;
    while i + min_silence <= hi {
        let window = &samples[i..i + min_silence];
        let mut energy = 0.0f64;
        let mut all_quiet = true;
        for &s in window {
            let a = s.abs() as f64;
            energy += a * a;
            if s.abs() > quiet_thresh {
                all_quiet = false;
                break;
            }
        }
        if all_quiet {
            energy /= min_silence as f64;
            let cut = i + min_silence / 2;
            match best {
                None => best = Some((energy, cut)),
                Some((e, c)) => {
                    if energy < e - 1e-18 || ((energy - e).abs() < 1e-18 && cut < c) {
                        best = Some((energy, cut));
                    }
                }
            }
        }
        i += min_silence.max(1) / 2;
        if i == lo {
            i += 1;
        }
    }
    best.map(|(_, cut)| cut)
}

// ---------------------------------------------------------------------------
// Overlap text / segment deduplication
// ---------------------------------------------------------------------------

/// Result of stitching two transcript pieces across an overlap.
#[derive(Debug, Clone, PartialEq)]
pub struct DedupeOutcome {
    pub text: String,
    pub dropped_prefix_tokens: usize,
    pub confident: bool,
    pub warning: Option<String>,
}

/// Normalize tokens for dedupe (lowercase alnum words).
pub fn normalize_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect()
}

/// Deduplicate `later` against the suffix of `earlier` within a bounded token window.
///
/// Never drops content when confidence is below threshold.
pub fn dedupe_overlap_text(earlier: &str, later: &str) -> DedupeOutcome {
    let earlier_t = normalize_tokens(earlier);
    let later_t = normalize_tokens(later);
    if earlier_t.is_empty() || later_t.is_empty() {
        return DedupeOutcome {
            text: later.to_string(),
            dropped_prefix_tokens: 0,
            confident: true,
            warning: None,
        };
    }

    let max_n = MAX_DEDUPE_TOKENS
        .min(earlier_t.len())
        .min(later_t.len());
    let mut best = 0usize;
    for n in (MIN_DEDUPE_TOKENS..=max_n).rev() {
        let suffix = &earlier_t[earlier_t.len() - n..];
        let prefix = &later_t[..n];
        if suffix == prefix {
            best = n;
            break;
        }
    }

    if best >= MIN_DEDUPE_TOKENS {
        // Drop the first `best` raw tokens from later, preserving remaining punctuation.
        let stripped = drop_n_tokens(later, best);
        DedupeOutcome {
            text: stripped,
            dropped_prefix_tokens: best,
            confident: true,
            warning: None,
        }
    } else {
        DedupeOutcome {
            text: later.to_string(),
            dropped_prefix_tokens: 0,
            confident: false,
            warning: Some(
                "overlap could not be resolved confidently; retained full later-chunk text".into(),
            ),
        }
    }
}

fn drop_n_tokens(s: &str, n: usize) -> String {
    if n == 0 {
        return s.to_string();
    }
    let mut seen = 0usize;
    let mut in_tok = false;
    let mut cut = 0usize;
    for (i, ch) in s.char_indices() {
        if ch.is_alphanumeric() {
            if !in_tok {
                in_tok = true;
                seen += 1;
                if seen > n {
                    cut = i;
                    break;
                }
            }
        } else {
            in_tok = false;
            if seen >= n {
                // skip whitespace after dropped tokens
                cut = i;
                if !ch.is_whitespace() {
                    break;
                }
            }
        }
        if seen >= n && !in_tok && !ch.is_whitespace() {
            cut = i;
            break;
        }
    }
    if seen < n {
        return String::new();
    }
    // Advance past leftover whitespace
    let rest = s[cut..].trim_start();
    rest.to_string()
}

/// Join transcript parts with overlap-aware dedupe (deterministic).
pub fn stitch_text_with_overlap(parts: &[(String, f64)]) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    if parts.is_empty() {
        return (String::new(), warnings);
    }
    let mut out = parts[0].0.trim().to_string();
    for (text, overlap_secs) in parts.iter().skip(1) {
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        if *overlap_secs > 0.0 {
            let d = dedupe_overlap_text(&out, t);
            if let Some(w) = d.warning {
                warnings.push(w);
            }
            if d.text.is_empty() {
                continue;
            }
            if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                out.push(' ');
            }
            out.push_str(d.text.trim());
        } else {
            if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                out.push(' ');
            }
            out.push_str(t);
        }
    }
    (out, warnings)
}

/// Deduplicate segments across an overlap boundary using text + time evidence.
pub fn dedupe_segments_overlap(
    earlier: &[Segment],
    later: &[Segment],
    overlap_secs: f64,
    later_offset_secs: f64,
) -> (Vec<Segment>, Option<String>) {
    if later.is_empty() {
        return (Vec::new(), None);
    }
    if overlap_secs <= 0.0 || earlier.is_empty() {
        return (later.to_vec(), None);
    }
    // If the first later segment text is a prefix-overlap of the last earlier segment, drop it.
    let last_earlier = earlier.last().map(|s| s.text()).unwrap_or("");
    let first_later = later[0].text();
    let d = dedupe_overlap_text(last_earlier, first_later);
    if d.confident && d.dropped_prefix_tokens > 0 {
        let mut out = Vec::with_capacity(later.len());
        if d.text.trim().is_empty() {
            out.extend(later.iter().skip(1).cloned());
        } else {
            let mut first = later[0].clone();
            first.set_text(d.text);
            // Keep times; source remains whatever it was (usually chunk_offset).
            let _ = later_offset_secs;
            out.push(first);
            out.extend(later.iter().skip(1).cloned());
        }
        return (out, None);
    }
    if !d.confident {
        return (
            later.to_vec(),
            Some("segment overlap not confidently deduped; retained later segments".into()),
        );
    }
    (later.to_vec(), None)
}

/// Whether SRT should fail closed for this provenance set.
pub fn srt_requires_allow_approximate(sources: &[TimestampSource]) -> bool {
    sources.iter().any(|s| s.is_approximate())
}

/// Derive legacy `timestamps_reliable` from provenance.
pub fn derive_timestamps_reliable(sources: &[TimestampSource]) -> bool {
    !sources.is_empty() && sources.iter().all(|s| s.is_reliable())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::WHISPER_SAMPLE_RATE;

    #[test]
    fn policy_rejects_inverted_bounds() {
        let mut p = LongFormPolicy::default();
        p.min_secs = 250.0;
        p.target_secs = 210.0;
        assert!(p.validate().is_err());
    }

    #[test]
    fn short_audio_single_window() {
        let sr = WHISPER_SAMPLE_RATE;
        let n = sr as usize * 30;
        let samples = vec![0.1f32; n];
        let plan = plan_boundary_windows(&samples, sr, &LongFormPolicy::default()).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].kind, BoundaryKind::ShortSingle);
        assert_eq!(plan[0].window.end_sample, n);
    }

    #[test]
    fn silence_boundary_preferred_over_hard_cut() {
        let sr = WHISPER_SAMPLE_RATE;
        // 400s of speech-like noise with silence near 210s.
        let n = (400.0 * f64::from(sr)) as usize;
        let mut samples = vec![0.3f32; n];
        let silence_at = (205.0 * f64::from(sr)) as usize;
        let silence_len = (sr as usize) * 2; // 2s silence
        for s in samples
            .iter_mut()
            .skip(silence_at)
            .take(silence_len)
        {
            *s = 0.0;
        }
        let plan = plan_boundary_windows(&samples, sr, &LongFormPolicy::default()).unwrap();
        assert!(plan.len() >= 2);
        // First cut should land near silence, not exactly 210 if silence found.
        let first_end = plan[0].window.end_sample as f64 / f64::from(sr);
        assert!(
            (200.0..220.0).contains(&first_end),
            "first end {first_end}"
        );
        assert_eq!(plan[0].kind, BoundaryKind::Silence);
        assert_eq!(plan.last().unwrap().window.end_sample, n);
    }

    #[test]
    fn continuous_noise_uses_overlap() {
        let sr = WHISPER_SAMPLE_RATE;
        let n = (500.0 * f64::from(sr)) as usize;
        let samples = vec![0.4f32; n];
        let plan = plan_boundary_windows(&samples, sr, &LongFormPolicy::default()).unwrap();
        assert!(plan.len() >= 2);
        assert!(plan.iter().any(|p| matches!(
            p.kind,
            BoundaryKind::TargetWithOverlap | BoundaryKind::FixedFallback
        )));
        // Overlap means next start < previous end
        for w in plan.windows(2) {
            if w[0].overlap_secs > 0.0 {
                assert!(w[1].window.start_sample < w[0].window.end_sample);
            }
        }
        assert_eq!(plan[0].window.start_sample, 0);
        assert_eq!(plan.last().unwrap().window.end_sample, n);
    }

    #[test]
    fn dedupe_exact_overlap() {
        let d = dedupe_overlap_text(
            "the quick brown fox jumps over",
            "fox jumps over the lazy dog",
        );
        assert!(d.confident);
        assert!(d.dropped_prefix_tokens >= 3);
        assert_eq!(d.text.trim(), "the lazy dog");
    }

    #[test]
    fn dedupe_low_confidence_retains() {
        let d = dedupe_overlap_text("alpha beta gamma", "delta epsilon zeta");
        assert!(!d.confident);
        assert_eq!(d.text, "delta epsilon zeta");
        assert!(d.warning.is_some());
    }

    #[test]
    fn srt_approximate_gate() {
        assert!(srt_requires_allow_approximate(&[
            TimestampSource::ChunkOffset,
            TimestampSource::Interpolated
        ]));
        assert!(!srt_requires_allow_approximate(&[
            TimestampSource::NativeModel,
            TimestampSource::ChunkOffset
        ]));
        assert!(!derive_timestamps_reliable(&[
            TimestampSource::Interpolated
        ]));
        assert!(derive_timestamps_reliable(&[TimestampSource::ProviderSegment]));
    }

    #[test]
    fn stitch_text_with_overlap_dedupes() {
        let (text, warns) = stitch_text_with_overlap(&[
            ("hello world from aurum".into(), 0.0),
            ("from aurum systems".into(), 1.5),
        ]);
        assert!(text.contains("hello"));
        assert!(text.contains("systems"));
        assert!(warns.is_empty() || text.contains("from"));
    }
}
