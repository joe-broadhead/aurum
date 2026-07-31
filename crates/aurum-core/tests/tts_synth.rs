//! TTS synthesis path — downloads the pinned KittenTTS pack on first run.
//!
//! Offline unit coverage for empty text always runs.
//! Full synth (network/cache) is ignored by default:
//!
//! ```bash
//! cargo test -p aurum-core --test tts_synth -- --ignored --nocapture
//! # or: AURUM_TTS_INTEGRATION=1 cargo test -p aurum-core --test tts_synth -- --ignored
//! ```

#![cfg(feature = "tts")]

use aurum_core::tts::{
    write_wav_i16_mono_atomic, LocalTtsProvider, SynthesisOptions, SynthesisProvider,
    DEFAULT_TTS_MODEL, DEFAULT_TTS_VOICE,
};
use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    // Prefer shared platform cache so CLI smoke and tests share the pack.
    aurum_core::Config::default_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("aurum-tts-test-cache"))
}

#[tokio::test]
async fn empty_text_is_user_error() {
    let provider = LocalTtsProvider::new(cache_dir()).with_local_only(true);
    let err = provider
        .synthesize("", &SynthesisOptions::default())
        .await
        .unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[tokio::test]
#[ignore = "downloads voice pack on first run; enable with --ignored or AURUM_TTS_INTEGRATION=1"]
async fn synth_fixed_phrase_to_wav() {
    let cache = cache_dir();
    std::fs::create_dir_all(&cache).ok();

    let local_only = std::env::var("AURUM_TTS_LOCAL_ONLY").ok().as_deref() == Some("1");
    let provider = LocalTtsProvider::new(cache)
        .with_progress(true)
        .with_local_only(local_only);

    let opts = SynthesisOptions {
        model: DEFAULT_TTS_MODEL.into(),
        voice: DEFAULT_TTS_VOICE.into(),
        language: "en".into(),
        sample_rate_hz: None,
        speaking_rate: 1.0,
        timeout_ms: 180_000,
        cancel: None,
        local_only,
        pack_dir: None,
        allow_unverified: false,
    };

    let result = provider
        .synthesize("Aurum TTS test.", &opts)
        .await
        .expect("synthesize");

    assert_eq!(result.channels, 1);
    assert!(result.duration_ms > 0, "duration_ms should be > 0");
    assert!(
        result.pcm_i16_mono.len() > 1000,
        "expected a non-trivial PCM buffer, got {}",
        result.pcm_i16_mono.len()
    );
    assert_eq!(result.provider, "local");
    assert_eq!(result.model, DEFAULT_TTS_MODEL);
    assert_eq!(result.voice, DEFAULT_TTS_VOICE);
    assert_eq!(result.sample_rate_hz, 24_000);
    assert!(!result.text_truncated);
    assert!(result.chunk_count >= 1);
    assert_eq!(result.synthesized_chars, result.text_chars);
    let expected_ms =
        (result.pcm_i16_mono.len() as u64).saturating_mul(1000) / result.sample_rate_hz as u64;
    assert_eq!(result.duration_ms, expected_ms);

    let out = std::env::temp_dir().join("aurum-tts-integration.wav");
    write_wav_i16_mono_atomic(&out, &result.pcm_i16_mono, result.sample_rate_hz)
        .expect("write wav");
    let meta = std::fs::metadata(&out).expect("meta");
    assert!(meta.len() > 44, "wav file too small: {}", meta.len());

    let reader = hound::WavReader::open(&out).expect("open wav");
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(reader.spec().sample_rate, result.sample_rate_hz);
    assert_eq!(reader.spec().sample_format, hound::SampleFormat::Int);
    assert_eq!(reader.spec().bits_per_sample, 16);
}

#[tokio::test]
#[ignore = "downloads voice pack on first run; enable with --ignored or AURUM_TTS_INTEGRATION=1"]
async fn synth_long_text_is_chunked_into_one_result() {
    let cache = cache_dir();
    std::fs::create_dir_all(&cache).ok();
    let local_only = std::env::var("AURUM_TTS_LOCAL_ONLY").ok().as_deref() == Some("1");
    let provider = LocalTtsProvider::new(cache)
        .with_progress(true)
        .with_local_only(local_only)
        .with_max_chars(10_000);

    let text = "Tadej Pogačar (born 21 September 1998), nicknamed \"Pogi\", is a \
            Slovenian professional cyclist who rides for UCI WorldTeam UAE Team Emirates XRG. \
            His victories include five Tours de France (2020, 2021, 2024, 2025 and 2026), the \
            2024 Giro d'Italia, and thirteen one-day Monuments (Milan–San Remo once, Tour of \
            Flanders three times, Liège–Bastogne–Liège four times and Giro di Lombardia five \
            times), as well as the World Championship Road Race twice. Comfortable in \
            time-trialing, one-day classic riding and grand-tour climbing, he has been compared \
            to all-round cyclists such as Eddy Merckx and Bernard Hinault. Despite his youth, he \
            is considered one of the greatest cyclists of all time.";

    let opts = SynthesisOptions {
        model: DEFAULT_TTS_MODEL.into(),
        voice: DEFAULT_TTS_VOICE.into(),
        language: "en".into(),
        sample_rate_hz: None,
        speaking_rate: 1.0,
        timeout_ms: 300_000,
        cancel: None,
        local_only,
        pack_dir: None,
        allow_unverified: false,
    };

    let result = provider.synthesize(text, &opts).await.expect("long synth");
    assert!(
        result.chunk_count > 1,
        "expected multi-chunk synthesis, got {}",
        result.chunk_count
    );
    assert!(!result.text_truncated);
    assert_eq!(result.sample_rate_hz, 24_000);
    assert!(result.pcm_i16_mono.len() > 24_000);
    assert_eq!(
        result.duration_ms,
        (result.pcm_i16_mono.len() as u64).saturating_mul(1000) / 24_000
    );

    let out = std::env::temp_dir().join("aurum-tts-long-chunk.wav");
    write_wav_i16_mono_atomic(&out, &result.pcm_i16_mono, result.sample_rate_hz)
        .expect("write wav");
    let reader = hound::WavReader::open(&out).expect("open wav");
    assert_eq!(reader.spec().sample_rate, 24_000);
    assert_eq!(reader.spec().channels, 1);
}
