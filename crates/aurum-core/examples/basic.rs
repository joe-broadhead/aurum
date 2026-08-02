//! Minimal library usage example.
//!
//!   cargo run -p aurum-core --example basic -- path/to/audio.wav
//!
//! Example path: `crates/aurum-core/examples/basic.rs`

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
                cancel: None,
            },
        )
        .await?;
    println!("{}", result.text());
    // Required on macOS/Metal: drop cached contexts before process exit.
    aurum_core::providers::local::clear_context_cache();
    Ok(())
}
