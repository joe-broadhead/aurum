# aurum-core

Reusable **on-device speech-to-text** library for [Aurum](https://github.com/joe-broadhead/aurum).

- Local whisper.cpp provider (`whisper-rs`)
- Optional OpenRouter (LLM-assisted) provider
- Audio load/convert, model cache, txt/srt/json formatters

**API status:** experimental until `0.1.0`. Prefer a pinned git `rev` or tag.

## Use in another crate

```toml
# path
aurum-core = { path = "../aurum/crates/aurum-core" }

# git (pin the rev!)
aurum-core = { git = "https://github.com/joe-broadhead/aurum", package = "aurum-core", rev = "…" }
```

```rust
use aurum_core::audio::load_audio;
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider};

# async fn demo() -> aurum_core::Result<()> {
let audio = load_audio(std::path::Path::new("meeting.m4a")).await?;
let provider = LocalWhisperProvider::new(std::path::PathBuf::from("/tmp/aurum-cache"));
let result = provider.transcribe(&audio, &TranscriptionOptions {
    model: "tiny-q5_1".into(),
    language: "en".into(),
    timestamps: true,
}).await?;
println!("{}", result.text);
aurum_core::providers::local::clear_context_cache();
# Ok(())
# }
```

Full docs: <https://joe-broadhead.github.io/aurum/library/integration/>

## Build requirements

cmake + C/C++ toolchain (whisper-rs). ffmpeg at runtime for non-WAV inputs.

## License

MIT
