//! Local provider integration test.
//!
//! Ignored by default (downloads a ~75 MB model). Run with:
//!   cargo test --test local_integration -- --ignored --nocapture
//!
//! Or set AURUM_INTEGRATION=1 in CI.

use aurum::audio::{self, write_temp_wav};
use aurum::output::{format_result, OutputFormat};
use aurum::providers::{LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider};
use std::path::PathBuf;
use tempfile::tempdir;

fn should_run() -> bool {
    std::env::var_os("AURUM_INTEGRATION").is_some() || std::env::args().any(|a| a == "--ignored")
}

/// Generate a short spoken-like tone wav (not real speech — validates pipeline wiring).
/// For a stronger test we rely on whisper still returning *something* or empty text
/// without crashing.
fn make_noise_wav(path: &std::path::Path) {
    // 2 seconds of quiet + a simple tone burst — enough to exercise decode/inference.
    let mut samples = vec![0.0f32; 16_000 * 2];
    for (i, s) in samples.iter_mut().enumerate().take(16_000).skip(8_000) {
        let t = i as f32 / 16_000.0;
        *s = (2.0 * std::f32::consts::PI * 220.0 * t).sin() * 0.1;
    }
    write_temp_wav(&samples, path).expect("write wav");
}

#[tokio::test]
#[ignore = "downloads tiny model (~75MB); run explicitly or with AURUM_INTEGRATION=1"]
async fn local_tiny_pipeline() {
    if !should_run() && std::env::var_os("AURUM_INTEGRATION").is_none() {
        // `cargo test -- --ignored` still runs this; the ignore attribute gates default runs.
    }

    let dir = tempdir().unwrap();
    let wav = dir.path().join("sample.wav");
    make_noise_wav(&wav);

    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();

    let audio = audio::load_audio(&wav).await.expect("load audio");
    assert!(audio.duration_secs > 1.0);

    let provider = LocalWhisperProvider::new(cache).with_progress(true);
    let result = provider
        .transcribe(
            &audio,
            &TranscriptionOptions {
                model: "tiny".into(),
                language: "en".into(),
                timestamps: true,
            },
        )
        .await
        .expect("transcribe");

    assert_eq!(result.provider, "local");
    assert_eq!(result.model, "tiny");
    // Text may be empty for non-speech audio; just ensure formatters work.
    let txt = format_result(&result, OutputFormat::Txt).unwrap();
    let srt = format_result(&result, OutputFormat::Srt).unwrap();
    let json = format_result(&result, OutputFormat::Json).unwrap();
    let _ = (txt, srt, json);

    // Model should now be cached.
    let model_file: PathBuf = dir
        .path()
        .join("cache")
        .join("models")
        .join("ggml-tiny.bin");
    assert!(
        model_file.exists(),
        "expected cached model at {}",
        model_file.display()
    );
}
