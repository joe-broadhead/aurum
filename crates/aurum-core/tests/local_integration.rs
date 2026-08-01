//! Local provider integration test with real speech fixture.
//!
//! Run with:
//!   cargo test -p aurum-core --test local_integration -- --ignored --nocapture
//!
//! Or set AURUM_INTEGRATION=1 (CI).

use aurum_core::audio;
use aurum_core::output::{format_result, OutputFormat};
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn sample_wav() -> PathBuf {
    // Prefer repo fixture (real TTS speech).
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample.wav"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.wav"),
        PathBuf::from("tests/fixtures/sample.wav"),
    ];
    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    panic!("sample.wav fixture not found");
}

#[tokio::test]
#[ignore = "downloads model on first run; enable with AURUM_INTEGRATION=1 or --ignored"]
async fn local_tiny_q5_real_speech() {
    let wav = sample_wav();
    assert!(wav.exists(), "missing {}", wav.display());

    // Prefer shared user cache so CI/dev don't re-download every run.
    let cache = aurum_core::config::Config::default_cache_dir()
        .unwrap_or_else(|_| tempdir().unwrap().path().join("cache"));
    std::fs::create_dir_all(&cache).ok();

    let audio = audio::load_audio(&wav).await.expect("load audio");
    assert!(audio.duration_secs() > 1.0);

    let provider = LocalWhisperProvider::new(cache.clone()).with_progress(true);
    let opts = TranscriptionOptions {
        // Small quantized model — faster first-run download (~32 MB).
        model: "tiny-q5_1".into(),
        language: "en".into(),
        timestamps: true,
        cancel: None,
    };

    let result = provider
        .transcribe(&audio, &opts)
        .await
        .expect("transcribe");

    assert_eq!(result.provider(), "local");
    assert_eq!(result.model(), "tiny-q5_1");
    assert!(result.timestamps_reliable());
    assert!(
        !result.text().trim().is_empty(),
        "expected non-empty transcript, got {:?}",
        result.text()
    );
    // Real fixture says something about a test / aurum / numbers.
    let lower = result.text().to_ascii_lowercase();
    assert!(
        lower.contains("test")
            || lower.contains("hello")
            || lower.contains("one")
            || lower.contains("1"),
        "unexpected transcript content: {}",
        result.text()
    );

    let _ = format_result(&result, OutputFormat::Txt).unwrap();
    let srt = format_result(&result, OutputFormat::Srt).unwrap();
    assert!(srt.contains("-->"), "srt missing cues");
    let json = format_result(&result, OutputFormat::Json).unwrap();
    assert!(json.contains("timestamps_reliable"));

    // Second call should reuse process-level context cache (smoke: still works).
    let result2 = provider
        .transcribe(&audio, &opts)
        .await
        .expect("second transcribe");
    assert!(!result2.text().trim().is_empty());

    let model_file = cache.join("models").join("ggml-tiny-q5_1.bin");
    assert!(
        model_file.exists() || find_model_in_cache(&cache),
        "expected cached quantized model under {}",
        cache.display()
    );

    // Drop Metal-backed contexts before process exit (avoids ggml-metal assert).
    aurum_core::providers::local::clear_context_cache();
}

fn find_model_in_cache(cache: &Path) -> bool {
    cache.join("models").join("ggml-tiny-q5_1.bin").exists()
}
