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

## Minimal example (file)

```rust
use aurum_core::audio::load_audio;
use aurum_core::config::Config;
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};

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
    aurum_core::providers::local::clear_context_cache();
    Ok(())
}
```

## PCM / mic host (no ffmpeg)

```rust
use aurum_core::pcm::PcmBuffer;
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> aurum_core::Result<()> {
    let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"))
        .with_progress(false)
        // Fail closed if model not already downloaded:
        .with_local_only(true)
        .with_download_progress(Arc::new(|p| {
            if let Some(f) = p.fraction() {
                eprintln!("download {} {:.0}%", p.model, f * 100.0);
            }
        }));

    // Startup (use local_only=false once to fetch):
    // provider.with_local_only(false).preload("tiny-q5_1").await?;
    if provider.is_model_cached("tiny-q5_1") {
        provider.preload("tiny-q5_1").await?;
    }

    let mut buf = PcmBuffer::dictation(); // ~60s rolling @ 16 kHz
    // on each mic callback:
    buf.push(&/* [f32] @ 16 kHz mono */ vec![0.0; 512])?;

    let result = provider
        .transcribe_pcm(
            buf.samples(),
            &TranscriptionOptions {
                model: "tiny-q5_1".into(),
                language: "en".into(),
                timestamps: false,
            },
        )
        .await?;
    println!("{}", result.text);
    aurum_core::providers::local::clear_context_cache();
    Ok(())
}
```

| API | Use |
|-----|-----|
| `AudioInput::from_pcm` / `from_pcm_slice` | Wrap existing 16 kHz mono f32 |
| `PcmBuffer` | Accumulate mic chunks (rolling or bounded) |
| `LocalWhisperProvider::transcribe_pcm` | Finalize without files |
| `preload` | Load ggml into process cache at startup |
| `with_local_only(true)` | No network if model missing |
| `with_download_progress` | UI progress hook |

## Host-oriented notes

| Concern | Guidance |
|---------|----------|
| Sample rate | **16 kHz mono f32 only** — resample in the host |
| Long-lived process | Reuse one provider; context cache is process-global |
| Shutdown | Always `clear_context_cache()` before exit (Metal) |
| Offline / Local Only | `with_local_only(true)` + never construct OpenRouter |
| Models | Prefer quantized (`tiny-q5_1` / `base-q5_1`) for first download |
| Threading | Inference runs on `spawn_blocking` |
| Partials | Not in-core yet — host can call `transcribe_pcm` on a window |

## System deps for consumers

Linking `aurum-core` still needs **cmake** + a C++ toolchain at **build** time
(for whisper-rs). End users of a fully static product binary do not need Rust,
but your CI image does.

ffmpeg is required at **runtime** for non-16 kHz-mono-WAV inputs.
