# Integrating aurum-core

## Cargo dependency

### Path (monorepo / local checkout)

```toml
[dependencies]
aurum-core = { path = "../aurum/crates/aurum-core" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

### Git (private or public)

```toml
[dependencies]
aurum-core = { git = "https://github.com/joe-broadhead/aurum", package = "aurum-core", rev = "REPLACE_WITH_SHA" }
```

Use a **pinned `rev`** (or tag once releases exist). Avoid floating `branch = "main"` in shipping apps.

### Optional: workspace member

```toml
# consumer Cargo.toml
[workspace]
members = ["app", "…"]

# if you vendor aurum as a submodule:
# members = ["app", "vendor/aurum/crates/aurum-core"]
```

## Minimal example

```rust
use aurum_core::audio::load_audio;
use aurum_core::config::Config;
use aurum_core::providers::{
    LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider,
};

#[tokio::main]
async fn main() -> aurum_core::Result<()> {
    let cfg = Config::load()?;
    let audio = load_audio(std::path::Path::new("meeting.m4a")).await?;
    let provider = LocalWhisperProvider::new(cfg.cache_dir).with_progress(false);
    let result = provider
        .transcribe(
            &audio,
            &TranscriptionOptions {
                model: "tiny-q5_1".into(),
                language: "en".into(),
                timestamps: true,
            },
        )
        .await?;
    println!("{}", result.text);

    // Required on macOS/Metal before process exit:
    aurum_core::providers::local::clear_context_cache();
    Ok(())
}
```

## ZephyrFlow-oriented notes

| Concern | Guidance |
|---------|----------|
| Long-lived process | Reuse one `LocalWhisperProvider`; context cache is process-global |
| Shutdown | Always `clear_context_cache()` before exit (Metal) |
| Offline | Stick to `local`; never construct `OpenRouterProvider` on Local Only paths |
| Models | Prefer quantized (`tiny-q5_1` / `base-q5_1`) for first download UX |
| Threading | `transcribe` is async but runs whisper on `spawn_blocking` |

## System deps for consumers

Linking `aurum-core` still needs **cmake** + a C++ toolchain at **build** time
(for whisper-rs). End users of a fully static product binary do not need Rust,
but your CI image does.

ffmpeg is required at **runtime** for non-16 kHz-mono-WAV inputs.
