# aurum-core

Reusable **on-device speech I/O** library for [Aurum](https://github.com/joe-broadhead/aurum).

**Speech both ways. On-device by default.**

- Local whisper.cpp STT (`whisper-rs`, Metal on macOS)
- Local ONNX TTS (KittenTTS; feature `tts`, default on)
- Optional OpenRouter (LLM-assisted ASR / cleanup)
- PCM-first mic host API · partial-window helpers · cancel
- Cleanup / flow (rules or OpenRouter)
- Model download, pins, txt/srt/json

**API status:** experimental until `0.1.0`. Pin a git `rev` or tag.

## Depend

```toml
aurum-core = "0.0.8"
# STT only (smaller): default-features = false
# git: tag = "v0.0.8"
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
            timestamps: true,
            cancel: None,
        },
    )
    .await?;
println!("{}", result.text);
aurum_core::providers::local::clear_context_cache();
# Ok(())
# }
```

Docs: <https://joe-broadhead.github.io/aurum/library/integration/>

## Build requirements

- **Build:** cmake + C/C++ toolchain  
- **Runtime (STT files):** ffmpeg for non-16 kHz mono WAV  
- **TTS:** ONNX Runtime via `ort` (feature `tts`)

## License

MIT
