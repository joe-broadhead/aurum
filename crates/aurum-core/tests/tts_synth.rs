//! TTS synthesis path — downloads the pinned KittenTTS pack on first run.
//!
//! Run with network once to warm cache:
//!   cargo test -p aurum-core --test tts_synth -- --nocapture

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
async fn synth_fixed_phrase_to_wav() {
    let cache = cache_dir();
    let provider = LocalTtsProvider::new(cache)
        .with_progress(true)
        .with_local_only(false);

    let opts = SynthesisOptions {
        model: DEFAULT_TTS_MODEL.into(),
        voice: DEFAULT_TTS_VOICE.into(),
        language: "en".into(),
        sample_rate_hz: None,
        speaking_rate: 1.0,
        timeout_ms: 180_000,
        cancel: None,
        local_only: false,
    };

    let result = provider
        .synthesize("Aurum TTS test.", &opts)
        .await
        .expect("synthesize");

    assert_eq!(result.channels, 1);
    assert!(result.duration_ms > 0, "duration_ms should be > 0");
    assert!(
        result.pcm_i16_mono.len() > 44,
        "expected more than a header's worth of samples"
    );
    assert_eq!(result.provider, "local");
    assert_eq!(result.model, DEFAULT_TTS_MODEL);
    assert_eq!(result.voice, DEFAULT_TTS_VOICE);

    let out = std::env::temp_dir().join("aurum-tts-integration.wav");
    write_wav_i16_mono_atomic(&out, &result.pcm_i16_mono, result.sample_rate_hz)
        .expect("write wav");
    let meta = std::fs::metadata(&out).expect("meta");
    assert!(meta.len() > 44, "wav file too small: {}", meta.len());

    let reader = hound::WavReader::open(&out).expect("open wav");
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(reader.spec().sample_rate, result.sample_rate_hz);
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
