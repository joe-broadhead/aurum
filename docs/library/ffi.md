# Native embeds (`aurum-ffi`)

C ABI façade over `aurum-core` for Swift, Kotlin, C, C#, and other hosts that
should not shell out to the CLI.

**On-device only** in this crate: no OpenRouter, no mic capture, no streaming loop.
Partials and hold-to-talk UX stay in the host.

| | |
|--|--|
| Header | [`crates/aurum-ffi/include/aurum.h`](https://github.com/joe-broadhead/aurum/blob/master/crates/aurum-ffi/include/aurum.h) |
| Crate | `aurum-ffi` (workspace; not required for CLI users) |
| Sample rate | **16 000 Hz** mono `float32` |
| ABI | `AURUM_ABI_VERSION` (currently `1`; additive status codes `BUSY`/`DEADLINE`/`OVERLOAD`) |

## Build

```bash
cargo build -p aurum-ffi --release
# → target/release/libaurum_ffi.{a,dylib,so} or aurum_ffi.dll
# link with include/aurum.h
```

Rust hosts can use the same façade without C:

```rust
use aurum_ffi::{cleanup_rules, CleanupStyle, Engine, EngineConfig, TranscribeOpts};

let engine = Engine::new(EngineConfig {
    cache_dir: cache_dir.into(),
    local_only: true,
    progress_logging: false,
})?;
if engine.is_model_ready("tiny-q5_1") {
    engine.preload("tiny-q5_1")?;
}
let t = engine.transcribe_pcm(
    &pcm, // mono f32 @ 16 kHz
    &TranscribeOpts {
        model: "tiny-q5_1".into(),
        language: "en".into(),
        timestamps: false,
    },
)?;
let cleaned = cleanup_rules(&t.text, CleanupStyle::Clean)?;
drop(engine);
aurum_ffi::shutdown(); // before process exit (Metal-safe)
```

## C surface (summary)

| Call | Role |
|------|------|
| `aurum_engine_create` / `destroy` | Handle + cache dir; `local_only` default recommended |
| `aurum_engine_preload` | Download (unless local_only) + warm context |
| `aurum_engine_is_model_ready` | Cache probe (read-only) |
| `aurum_engine_transcribe_pcm` | Decode |
| `aurum_engine_cancel` | Cooperative cancel (other thread OK) |
| `aurum_cleanup_rules` | On-device text cleanup (no engine required) |
| `aurum_shutdown` / `aurum_shutdown_ex` | Drain exclusive ops; clear whisper cache only when idle (`BUSY` if still active) |

Zero-initialize config/opts structs (`reserved` must be `0`).

## Threading

- **One** exclusive op (`preload` or `transcribe_pcm`) **per engine** at a time.
- **Distinct engines** may run concurrently.
- Do **not** call blocking FFI from inside a host Tokio/async task (nested `block_on`).
- `aurum_engine_last_error` pointer is valid only until the next call on the **same thread** — copy immediately.

## Host checklist

| Concern | Guidance |
|---------|----------|
| Resample | Host must deliver 16 kHz mono f32 |
| First run | `is_model_ready` → UX → `preload` |
| Offline | `local_only = 1` |
| Cancel | `aurum_engine_cancel` mid-hold |
| Cleanup | Separate stage after ASR (`cleanup_rules`) |
| Exit | `destroy` all engines, then `aurum_shutdown` / `aurum_shutdown_ex` (prefer the latter if you need `BUSY`) |
| Partials | Host loop + rolling PCM window; see [Partials](partials.md) |

## Related

- [Library overview](overview.md)  
- [Rust integration](integration.md)  
- [Architecture](../development/architecture.md)  
