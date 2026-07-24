//! PCM-first API unit tests (no model download).

use aurum_core::audio::{AudioInput, WHISPER_SAMPLE_RATE};
use aurum_core::error::{TranscriptionError, UserError};
use aurum_core::model::{ensure_model_with_options, EnsureModelOptions};
use aurum_core::pcm::PcmBuffer;
use aurum_core::providers::LocalWhisperProvider;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn from_pcm_rejects_wrong_rate() {
    let err = AudioInput::from_pcm_slice(&[0.0; 100], 44_100).unwrap_err();
    assert!(matches!(
        err,
        TranscriptionError::User(UserError::UnsupportedSampleRate { got: 44_100, .. })
    ));
}

#[test]
fn from_pcm_ok() {
    let audio = AudioInput::from_pcm_slice(&[0.1; 1600], WHISPER_SAMPLE_RATE).unwrap();
    assert_eq!(audio.sample_rate, 16_000);
    assert!((audio.duration_secs - 0.1).abs() < 1e-6);
    assert!(audio.source_path.to_string_lossy().starts_with("pcm://"));
}

#[test]
fn pcm_buffer_roundtrip() {
    let mut buf = PcmBuffer::dictation();
    buf.push(&[0.0; 800]).unwrap();
    buf.push(&[0.5; 800]).unwrap();
    assert_eq!(buf.len(), 1600);
    let audio = buf.to_audio_input().unwrap();
    assert_eq!(audio.len(), 1600);
}

#[tokio::test]
async fn local_only_fails_when_missing() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();

    let err = ensure_model_with_options(
        &cache,
        "tiny-q5_1",
        EnsureModelOptions::new().local_only(true),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        TranscriptionError::User(UserError::ModelNotCached { .. })
    ));

    let provider = LocalWhisperProvider::new(cache)
        .with_progress(false)
        .with_local_only(true);
    assert!(!provider.is_model_cached("tiny-q5_1"));
    let err = provider.preload("tiny-q5_1").await.unwrap_err();
    assert!(matches!(
        err,
        TranscriptionError::User(UserError::ModelNotCached { .. })
    ));
}

#[tokio::test]
async fn transcribe_pcm_empty_errors() {
    let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-pcm-test"))
        .with_progress(false)
        .with_local_only(true);
    let err = provider
        .transcribe_pcm(&[], &Default::default())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        TranscriptionError::User(UserError::InvalidAudio { .. })
    ));
}
