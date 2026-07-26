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
