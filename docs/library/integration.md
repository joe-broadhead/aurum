# Integrating aurum-core

## Cargo dependency

```toml
# Path (local checkout)
aurum-core = { path = "../aurum/crates/aurum-core" }

# Git — always pin a rev or tag
aurum-core = { git = "https://github.com/joe-broadhead/aurum", package = "aurum-core", tag = "v0.0.22" }
```

Also need a Tokio runtime for async APIs:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## File transcription

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
                cancel: None,
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

#[tokio::main]
async fn main() -> aurum_core::Result<()> {
    let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"))
        .with_progress(false)
        .with_local_only(true); // fail closed if model not cached

    if provider.is_model_cached("tiny-q5_1") {
        provider.preload("tiny-q5_1").await?;
    }

    let mut buf = PcmBuffer::dictation(); // ~60s rolling @ 16 kHz
    buf.push(&/* mic chunk: [f32] @ 16 kHz mono */ vec![0.0; 512])?;

    let result = provider
        .transcribe_pcm(
            buf.samples().as_slice(),
            &TranscriptionOptions {
                model: "tiny-q5_1".into(),
                language: "en".into(),
                timestamps: false,
                cancel: None,
            },
        )
        .await?;
    println!("{}", result.text);
    aurum_core::providers::local::clear_context_cache();
    Ok(())
}
```

For interim text and cancel, see [Partials & cancel](partials.md).

## Cleanup after ASR

```rust
use aurum_core::cleanup::{
    apply_cleanup_with_segments, CleanupStyle, RulesCleanup, SegmentCleanupPolicy, TextCleanup,
};

# async fn demo(mut result: aurum_core::TranscriptionResult) -> aurum_core::Result<()> {
let rules = RulesCleanup::new();
// Returns (CleanupResult, CleanupReport). Mutates only on full success.
let (_out, _report) = apply_cleanup_with_segments(
    &mut result,
    &rules as &dyn TextCleanup,
    CleanupStyle::Clean,
    SegmentCleanupPolicy::Auto,
)
.await?;
# Ok(())
# }
```

## Host checklist

| Concern | Guidance |
|---------|----------|
| Sample rate | **16 kHz mono f32** — resample in the host |
| Long-lived process | Reuse one provider; context cache is process-global |
| Shutdown | Always `clear_context_cache()` before exit (Metal) |
| Offline | `with_local_only(true)`; never construct OpenRouter |
| Models | Prefer `tiny-q5_1` / `base-q5_1` for first download |
| Threading | Inference runs on `spawn_blocking` |
| Build | cmake + C++ toolchain at **build** time; ffmpeg at **runtime** for files |

## Native hosts

Prefer **[`aurum-ffi`](ffi.md)** (C ABI) instead of shelling out to the CLI.
Rust hosts may use `aurum_ffi::Engine` or call `aurum-core` directly as above.
