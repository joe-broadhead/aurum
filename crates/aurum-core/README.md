# aurum-core

Reusable **on-device speech I/O** library for [Aurum](https://github.com/joe-broadhead/aurum).

**Speech both ways. On-device by default.**

- Local whisper.cpp STT (`whisper-rs`, Metal on macOS)
- Local ONNX TTS (KittenTTS / Kokoro; feature `tts`, default on)
- Optional remote providers (OpenRouter / OpenAI / ElevenLabs / xAI)
- PCM-first mic host API · partial-window helpers · cancel
- Cleanup / flow (rules or OpenRouter)
- Model download, pins, txt/srt/json

**API status:** provisional on continuous **0.0.x** (next assurance cut **v0.0.22**, not 1.0). Pin a crates.io version or git tag.

## Depend

```toml
aurum-core = "0.0.22"
# STT only (smaller): default-features = false
# git: tag = "v0.0.22"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Quick STT example

```rust
use aurum_core::audio::load_audio;
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};

# async fn demo() -> aurum_core::Result<()> {
let audio = load_audio(std::path::Path::new("meeting.m4a")).await?;
let provider = LocalWhisperProvider::new(std::path::PathBuf::from("/tmp/aurum-cache"))
    .with_progress(false);
let result = provider
    .transcribe(
        &audio,
        &TranscriptionOptions {
            model: "tiny-q5_1".into(),
            language: "en".into(),
            timestamps: false,
            cancel: None,
        },
    )
    .await?;
println!("{}", result.text());
aurum_core::providers::local::clear_context_cache();
# Ok(())
# }
```

Prefer `AurumEngine` for long-lived hosts (owned pools, governor, registry). See
the [integration guide](https://joe-broadhead.github.io/aurum/library/integration/).

## License

MIT
