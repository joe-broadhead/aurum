//! Minimal library usage example.
//!
//!   cargo run -p aurum-core --example basic -- path/to/audio.wav
//!
//! (example lives at workspace root; prefer depending on aurum-core directly)

use aurum_core::audio::load_audio;
use aurum_core::config::Config;
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider};
use std::env;

#[tokio::main]
async fn main() -> aurum_core::Result<()> {
    let path = env::args().nth(1).expect("usage: basic <audio-file>");
    let cfg = Config::load()?;
    let audio = load_audio(std::path::Path::new(&path)).await?;
    let provider = LocalWhisperProvider::new(cfg.cache_dir).with_progress(true);
    let result = provider
        .transcribe(
            &audio,
            &TranscriptionOptions {
                model: "tiny-q5_1".into(),
                language: "auto".into(),
                timestamps: false,
            },
        )
        .await?;
    println!("{}", result.text);
    Ok(())
}
