# aurum-ffi

C ABI façade over [`aurum-core`](https://crates.io/crates/aurum-core) for native embedders.

**Audio in. Text out. On-device by default.**

| Capability | API |
|------------|-----|
| Engine | `aurum_engine_create` / `destroy` |
| Models | `preload`, `is_model_ready` |
| Decode | `aurum_engine_transcribe_pcm` (16 kHz mono f32) |
| Cancel | `aurum_engine_cancel` |
| Cleanup | `aurum_cleanup_rules` (on-device, no network) |
| Teardown | `aurum_shutdown` (Metal-safe) |

Header: [`include/aurum.h`](include/aurum.h)

## Rust

```rust
use aurum_ffi::{cleanup_rules, CleanupStyle, Engine, EngineConfig, TranscribeOpts};

let engine = Engine::new(EngineConfig {
    cache_dir: cache.into(),
    local_only: true,
    progress_logging: false,
})?;
engine.preload("tiny-q5_1")?;
let t = engine.transcribe_pcm(&pcm, &TranscribeOpts {
    model: "tiny-q5_1".into(),
    language: "en".into(),
    timestamps: false,
})?;
let cleaned = cleanup_rules(&t.text, CleanupStyle::Clean)?;
aurum_ffi::shutdown();
```

## Build

```bash
cargo build -p aurum-ffi --release
# libaurum_ffi.{a,dylib,so,dll} + include/aurum.h
```

## Notes

- Partials / mic capture stay in the host.
- No OpenRouter in this crate (on-device path only).
- One in-flight `transcribe_pcm` per engine; concurrent calls return `AURUM_ERR_STATE`.

## License

MIT
