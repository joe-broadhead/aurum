//! Minimal library usage example.
//!
//!   cargo run --example basic -- path/to/audio.wav

use aurum::audio::load_audio;
use aurum::config::Config;
use aurum::providers::{LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider};
use std::env;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> aurum::Result<()> {
    let path = env::args().nth(1).expect("usage: basic <audio-file>");
    let cfg = Config::load()?;
    let audio = load_audio(std::path::Path::new(&path)).await?;
    let provider = LocalWhisperProvider::new(cfg.cache_dir).with_progress(true);
    let result = provider
        .transcribe(
            &audio,
            &TranscriptionOptions {
                model: "tiny".into(),
                language: "auto".into(),
                timestamps: false,
            },
        )
        .await?;
    println!("{}", result.text);
    let _ = PathBuf::new();
    Ok(())
}
