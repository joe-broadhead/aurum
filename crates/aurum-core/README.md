# aurum-core

Reusable **on-device speech-to-text** library for [Aurum](https://github.com/joe-broadhead/aurum).

**Audio in. Text out. On-device by default.**

- Local whisper.cpp (`whisper-rs`, Metal on macOS)
- Optional OpenRouter (LLM-assisted ASR)
- PCM-first mic host API · partial-window helpers · cancel
- Cleanup / flow (rules or OpenRouter)
- Model download, pins, txt/srt/json

**API status:** experimental until `0.1.0`. Pin a git `rev` or tag.

Optional **TTS** (default feature `tts`): `LocalTtsProvider` → mono PCM/WAV. Disable with `default-features = false` if you only need STT.

## Depend

```toml
aurum-core = { git = "https://github.com/joe-broadhead/aurum", package = "aurum-core", tag = "v0.0.0" }
# or: path = "../aurum/crates/aurum-core"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Quick example

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
- **Runtime (files):** ffmpeg for non-16 kHz mono WAV  

## License

MIT
