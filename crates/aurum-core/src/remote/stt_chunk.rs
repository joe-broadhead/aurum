//! Time-based remote STT chunk-and-stitch (JOE-2212).
//!
//! Long lectures can exceed [`TranscriptLimits::max_segment_chars`] when a vendor
//! returns a single continuous segment, or truncate on full-file remote paths.
//! Client-side audio chunking (~210s windows, dual-ref eval band) keeps each
//! request small and stitches text/segments with time offsets.
//!
//! Local whisper is unchanged (handles full-file natively).

use crate::audio::AudioInput;
use crate::error::{ProviderError, Result};
use crate::providers::{Segment, TranscriptionOptions, TranscriptionResult};
use crate::remote::limits::{validate_segments, validate_text_bounds, TranscriptLimits};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

/// Default remote STT window length (seconds). Matches Plaud dual-ref chunk recipes.
pub const DEFAULT_REMOTE_STT_CHUNK_SECS: f64 = 210.0;

/// Minimum duration that triggers chunking (must be > 0 and finite).
pub fn needs_time_chunk(duration_secs: f64, chunk_secs: f64) -> bool {
    duration_secs.is_finite()
        && chunk_secs.is_finite()
        && chunk_secs > 0.0
        && duration_secs > chunk_secs
}

/// Inclusive start / exclusive end sample ranges with start offset in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkWindow {
    pub start_sample: usize,
    pub end_sample: usize,
    pub offset_secs: f64,
}

/// Plan non-overlapping PCM windows covering `[0, total_samples)`.
pub fn plan_chunk_windows(
    total_samples: usize,
    sample_rate: u32,
    chunk_secs: f64,
) -> Vec<ChunkWindow> {
    if total_samples == 0 || sample_rate == 0 || !chunk_secs.is_finite() || chunk_secs <= 0.0 {
        return vec![ChunkWindow {
            start_sample: 0,
            end_sample: total_samples,
            offset_secs: 0.0,
        }];
    }
    let chunk_samples = ((chunk_secs * f64::from(sample_rate)).round() as usize).max(1);
    if total_samples <= chunk_samples {
        return vec![ChunkWindow {
            start_sample: 0,
            end_sample: total_samples,
            offset_secs: 0.0,
        }];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < total_samples {
        let end = (start + chunk_samples).min(total_samples);
        out.push(ChunkWindow {
            start_sample: start,
            end_sample: end,
            offset_secs: start as f64 / f64::from(sample_rate),
        });
        if end == total_samples {
            break;
        }
        start = end;
    }
    out
}

/// Slice mono PCM into a new [`AudioInput`] for one window.
pub fn slice_audio_window(input: &AudioInput, window: ChunkWindow) -> Result<AudioInput> {
    let samples = input.samples();
    let start = window.start_sample.min(samples.len());
    let end = window.end_sample.min(samples.len()).max(start);
    if start == end {
        return Err(ProviderError::Other {
            message: "remote STT chunk window is empty".into(),
        }
        .into());
    }
    let slice: Arc<[f32]> = Arc::from(samples[start..end].to_vec());
    let sr = input.sample_rate();
    let duration = if sr > 0 {
        (end - start) as f64 / f64::from(sr)
    } else {
        0.0
    };
    Ok(AudioInput::from_parts_unchecked(
        PathBuf::from(format!(
            "pcm://remote-stt-chunk/{:.3}-{:.3}",
            window.offset_secs,
            window.offset_secs + duration
        )),
        slice,
        sr,
        duration,
    ))
}

/// Soft-split an overlong single segment into adjacent pieces under `max_chars`.
///
/// Used when a vendor returns one continuous hyp for a chunk that still exceeds
/// [`TranscriptLimits::max_segment_chars`] (rare for ~210s speech, but fail-closed
/// without soft-split would block the whole job).
pub fn soft_split_text_segments(
    text: &str,
    start: f64,
    end: f64,
    max_chars: usize,
) -> Vec<Segment> {
    let max_chars = max_chars.max(1);
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![Segment::from_parts_unchecked(start, end, String::new())];
    }
    if chars.len() <= max_chars {
        return vec![Segment::from_parts_unchecked(start, end, text.to_string())];
    }
    let n_parts = chars.len().div_ceil(max_chars);
    let span = (end - start).max(0.0);
    let mut segs = Vec::with_capacity(n_parts);
    for i in 0..n_parts {
        let c0 = i * max_chars;
        let c1 = ((i + 1) * max_chars).min(chars.len());
        let piece: String = chars[c0..c1].iter().collect();
        let t0 = start + span * (c0 as f64 / chars.len() as f64);
        let t1 = start + span * (c1 as f64 / chars.len() as f64);
        segs.push(Segment::from_parts_unchecked(t0, t1.max(t0), piece));
    }
    segs
}

/// Offset segments by `offset_secs` and soft-split any that still exceed limits.
pub fn normalize_chunk_segments(
    segments: &[Segment],
    offset_secs: f64,
    limits: TranscriptLimits,
) -> Vec<Segment> {
    let mut out = Vec::new();
    for seg in segments {
        let start = seg.start() + offset_secs;
        let end = seg.end() + offset_secs;
        let text = seg.text();
        if text.chars().count() > limits.max_segment_chars {
            out.extend(soft_split_text_segments(
                text,
                start,
                end,
                limits.max_segment_chars,
            ));
        } else {
            out.push(Segment::from_parts_unchecked(start, end, text.to_string()));
        }
    }
    out
}

/// Join chunk results into one transcript for the full media duration.
pub fn stitch_chunk_results(
    parts: &[(f64, TranscriptionResult)],
    full_duration_secs: f64,
    provider: &str,
    limits: TranscriptLimits,
) -> Result<TranscriptionResult> {
    if parts.is_empty() {
        return Err(ProviderError::TranscriptionFailed {
            reason: "remote STT chunk-and-stitch produced no chunks".into(),
        }
        .into());
    }

    let mut texts: Vec<String> = Vec::with_capacity(parts.len());
    let mut segments: Vec<Segment> = Vec::new();
    let mut timestamps_reliable = true;
    let mut backend_kind = parts[0].1.backend_kind();
    let model = parts[0].1.model().to_string();
    let language = parts[0].1.language().map(|s| s.to_string());
    let provider_name = parts[0].1.provider().to_string();

    for (offset, r) in parts {
        let t = r.text().trim();
        if !t.is_empty() {
            texts.push(t.to_string());
        }
        timestamps_reliable &= r.timestamps_reliable();
        // Prefer ASR label if any chunk is dedicated ASR.
        if matches!(r.backend_kind(), crate::providers::BackendKind::Asr) {
            backend_kind = crate::providers::BackendKind::Asr;
        }
        segments.extend(normalize_chunk_segments(r.segments(), *offset, limits));
    }

    let text = join_transcript_parts(&texts);
    validate_text_bounds(&text, None, limits, provider)?;
    validate_segments(&segments, full_duration_secs, limits, provider)?;

    let mut result = TranscriptionResult::openrouter(
        text,
        segments,
        language,
        model,
        full_duration_secs,
        timestamps_reliable,
    );
    result.set_provider(if provider_name.is_empty() {
        provider
    } else {
        provider_name.as_str()
    });
    result.set_backend_kind(backend_kind);
    result.set_timestamps_reliable(timestamps_reliable);
    result.validate_segments()?;
    Ok(result)
}

fn join_transcript_parts(parts: &[String]) -> String {
    let mut out = String::new();
    for p in parts {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if !out.is_empty() && !out.ends_with(|c: char| c.is_whitespace()) {
            out.push(' ');
        }
        out.push_str(p);
    }
    out
}

/// Run `one_shot` once, or time-chunk + stitch when audio is longer than `chunk_secs`.
///
/// The callback receives **owned** inputs so futures do not borrow across awaits.
pub async fn transcribe_maybe_chunked<F, Fut>(
    input: &AudioInput,
    options: &TranscriptionOptions,
    provider: &str,
    chunk_secs: f64,
    mut one_shot: F,
) -> Result<TranscriptionResult>
where
    F: FnMut(AudioInput, TranscriptionOptions) -> Fut,
    Fut: Future<Output = Result<TranscriptionResult>>,
{
    let duration = input.duration_secs();
    if !needs_time_chunk(duration, chunk_secs) {
        return one_shot(input.clone(), options.clone()).await;
    }

    let windows = plan_chunk_windows(input.len(), input.sample_rate(), chunk_secs);
    tracing::info!(
        provider,
        duration_secs = duration,
        chunk_secs,
        chunks = windows.len(),
        "remote STT time-chunk-and-stitch (JOE-2212)"
    );

    let mut parts: Vec<(f64, TranscriptionResult)> = Vec::with_capacity(windows.len());
    for (i, window) in windows.iter().enumerate() {
        if let Some(flag) = options.cancel.as_ref() {
            if flag.is_cancelled() {
                return Err(ProviderError::Cancelled.into());
            }
        }
        let chunk_input = slice_audio_window(input, *window)?;
        tracing::debug!(
            provider,
            chunk = i + 1,
            of = windows.len(),
            offset_secs = window.offset_secs,
            chunk_duration = chunk_input.duration_secs(),
            "remote STT chunk"
        );
        let result = one_shot(chunk_input, options.clone()).await.map_err(|e| {
            // Surface which chunk failed for operator diagnostics (no secrets).
            match e {
                crate::error::TranscriptionError::Provider(
                    ProviderError::TranscriptionFailed { reason },
                ) => ProviderError::TranscriptionFailed {
                    reason: format!(
                        "chunk {}/{} (offset {:.1}s): {reason}",
                        i + 1,
                        windows.len(),
                        window.offset_secs
                    ),
                }
                .into(),
                other => other,
            }
        })?;
        parts.push((window.offset_secs, result));
    }

    stitch_chunk_results(&parts, duration, provider, TranscriptLimits::default())
}

/// Effective chunk length: `AURUM_REMOTE_STT_CHUNK_SECS` if set and valid, else default.
pub fn effective_chunk_secs() -> f64 {
    match std::env::var("AURUM_REMOTE_STT_CHUNK_SECS") {
        Ok(s) => {
            let v: f64 = s.trim().parse().unwrap_or(DEFAULT_REMOTE_STT_CHUNK_SECS);
            if v.is_finite() && v > 0.0 {
                v
            } else {
                DEFAULT_REMOTE_STT_CHUNK_SECS
            }
        }
        Err(_) => DEFAULT_REMOTE_STT_CHUNK_SECS,
    }
}

/// Convenience for tests: 16 kHz silence of `duration_secs`.
#[cfg(test)]
pub fn silence_input(duration_secs: f64) -> AudioInput {
    use crate::audio::WHISPER_SAMPLE_RATE;
    let n = (duration_secs * f64::from(WHISPER_SAMPLE_RATE)).round() as usize;
    AudioInput::from_pcm_slice(&vec![0.0f32; n.max(1)], WHISPER_SAMPLE_RATE).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::WHISPER_SAMPLE_RATE;
    use crate::providers::BackendKind;

    #[test]
    fn needs_chunk_threshold() {
        assert!(!needs_time_chunk(100.0, 210.0));
        assert!(!needs_time_chunk(210.0, 210.0));
        assert!(needs_time_chunk(210.1, 210.0));
        assert!(!needs_time_chunk(f64::NAN, 210.0));
    }

    #[test]
    fn plans_four_windows_for_685s() {
        let total = (685.0 * f64::from(WHISPER_SAMPLE_RATE)) as usize;
        let windows = plan_chunk_windows(total, WHISPER_SAMPLE_RATE, 210.0);
        assert_eq!(windows.len(), 4);
        assert_eq!(windows[0].offset_secs, 0.0);
        assert!((windows[1].offset_secs - 210.0).abs() < 0.01);
        assert_eq!(windows.last().unwrap().end_sample, total);
        // Coverage without gaps
        for w in windows.windows(2) {
            assert_eq!(w[0].end_sample, w[1].start_sample);
        }
    }

    #[test]
    fn single_window_when_short() {
        let total = WHISPER_SAMPLE_RATE as usize * 30;
        let windows = plan_chunk_windows(total, WHISPER_SAMPLE_RATE, 210.0);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].end_sample, total);
    }

    #[test]
    fn soft_split_respects_max_chars() {
        let text = "a".repeat(20);
        let segs = soft_split_text_segments(&text, 0.0, 10.0, 8);
        assert!(segs.len() > 1);
        assert!(segs.iter().all(|s| s.text().chars().count() <= 8));
        let joined: String = segs.iter().map(|s| s.text()).collect();
        assert_eq!(joined, text);
    }

    #[test]
    fn stitch_offsets_segments() {
        let mut a = TranscriptionResult::openrouter(
            "hello".to_string(),
            vec![Segment::from_parts_unchecked(0.0, 1.0, "hello".to_string())],
            None,
            "m".to_string(),
            1.0,
            true,
        );
        a.set_provider("openai");
        a.set_backend_kind(BackendKind::Asr);
        a.set_timestamps_reliable(true);

        let mut b = TranscriptionResult::openrouter(
            "world".to_string(),
            vec![Segment::from_parts_unchecked(0.0, 1.0, "world".to_string())],
            None,
            "m".to_string(),
            1.0,
            true,
        );
        b.set_provider("openai");
        b.set_backend_kind(BackendKind::Asr);
        b.set_timestamps_reliable(true);

        let stitched = stitch_chunk_results(
            &[(0.0, a), (210.0, b)],
            420.0,
            "openai",
            TranscriptLimits::default(),
        )
        .unwrap();
        assert_eq!(stitched.text(), "hello world");
        assert_eq!(stitched.segments().len(), 2);
        assert!((stitched.segments()[1].start() - 210.0).abs() < 1e-9);
        assert!(stitched.timestamps_reliable());
        assert_eq!(stitched.provider(), "openai");
    }

    #[tokio::test]
    async fn maybe_chunked_short_is_single_call() {
        let input = silence_input(5.0);
        let opts = TranscriptionOptions {
            model: "whisper-1".into(),
            language: "en".into(),
            timestamps: false,
            cancel: None,
        };
        let mut calls = 0u32;
        let out = transcribe_maybe_chunked(&input, &opts, "t", 210.0, |inp, _| {
            calls += 1;
            let mut r = TranscriptionResult::openrouter(
                "ok".to_string(),
                vec![Segment::from_parts_unchecked(
                    0.0,
                    inp.duration_secs(),
                    "ok".to_string(),
                )],
                None,
                "whisper-1".to_string(),
                inp.duration_secs(),
                false,
            );
            r.set_provider("t");
            async move { Ok(r) }
        })
        .await
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(out.text(), "ok");
    }

    #[tokio::test]
    async fn maybe_chunked_long_invokes_multiple() {
        let input = silence_input(500.0);
        let opts = TranscriptionOptions {
            model: "whisper-1".into(),
            language: "en".into(),
            timestamps: false,
            cancel: None,
        };
        let mut calls = 0u32;
        let out = transcribe_maybe_chunked(&input, &opts, "t", 210.0, |inp, _| {
            calls += 1;
            let label = format!("c{calls}");
            let mut r = TranscriptionResult::openrouter(
                label.clone(),
                vec![Segment::from_parts_unchecked(
                    0.0,
                    inp.duration_secs(),
                    label,
                )],
                None,
                "whisper-1".into(),
                inp.duration_secs(),
                false,
            );
            r.set_provider("t");
            async move { Ok(r) }
        })
        .await
        .unwrap();
        assert_eq!(calls, 3); // 210+210+80
        assert!(out.text().contains(' '));
        assert_eq!(out.segments().len(), 3);
        assert!((out.duration_secs() - 500.0).abs() < 0.02);
    }

    #[tokio::test]
    async fn maybe_chunked_honours_cancel() {
        let input = silence_input(500.0);
        let flag = crate::cancel::CancelFlag::new();
        flag.cancel();
        let opts = TranscriptionOptions {
            model: "whisper-1".into(),
            language: "en".into(),
            timestamps: false,
            cancel: Some(flag),
        };
        let err = transcribe_maybe_chunked(&input, &opts, "t", 210.0, |_inp, _| async {
            unreachable!("should cancel before first shot")
        })
        .await
        .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("cancel"));
    }
}
