//! Long-lived host: one engine, registry resolution, cancel, shutdown (JOE-1943).
//!
//! ```bash
//! cargo run -p aurum-core --example engine_providers -- path/to/audio.wav
//! ```
//!
//! Defaults remain local. With `OPENROUTER_API_KEY` / `OPENAI_API_KEY` / etc. set,
//! this example still uses `local` unless you change the config `provider` field.

use aurum_core::audio::load_audio;
use aurum_core::config::Config;
use aurum_core::{AurumEngine, ProviderId, TranscriptionOptions};
use std::env;

#[tokio::main]
async fn main() -> aurum_core::Result<()> {
    let path = env::args()
        .nth(1)
        .expect("usage: engine_providers <audio-file>");

    let cfg = Config::load()?;
    let engine = AurumEngine::from_config(cfg)?;

    // Credential-free discovery.
    let known = engine.registry().known_provider_hint();
    eprintln!("aurum: registered providers: {known}");

    let id = engine.stt_provider_id()?;
    eprintln!("aurum: using STT provider={}", id.as_str());

    let provider = engine.stt_provider(&id)?;
    let audio = load_audio(std::path::Path::new(&path)).await?;

    let cancel = aurum_core::CancelFlag::new();
    let opts = TranscriptionOptions {
        model: if id.as_str() == "local" {
            "tiny-q5_1".into()
        } else {
            // Remote paths require reviewed model ids for that provider.
            engine
                .config()
                .model
                .clone()
                .unwrap_or_else(|| "tiny-q5_1".into())
        },
        language: "en".into(),
        timestamps: false,
        cancel: Some(cancel.clone()),
        op: None,
    };

    let result = provider.transcribe(&audio, &opts).await?;
    println!("{}", result.text());
    eprintln!(
        "aurum: backend={:?} provider={}",
        result.backend_kind(),
        result.provider()
    );

    // Repeated call on the same engine (pools/governor stay owned).
    let _again = engine.stt_provider(&ProviderId::local())?.name();

    engine.shutdown();
    Ok(())
}
