# Embed paths (library + FFI)

Use when the host should **not** shell out to the CLI.

## Rust (`aurum-core`)

Prefer **`AurumEngine`** (owned config, governor, metrics, STT/TTS pools, provider registry):

```rust
use aurum_core::{AurumEngine, ProviderId, TranscriptionOptions};

let engine = AurumEngine::load()?;
let stt = engine.stt_provider(&ProviderId::local())?;
// Remote only with deliberate provider + secrets in ValidatedConfig:
// let stt = engine.stt_provider(&ProviderId::openrouter())?;

let opts = TranscriptionOptions {
    model: "tiny-q5_1".into(),
    language: "en".into(),
    timestamps: false,
    cancel: None,
};
// engine.transcribe_pcm(&samples, &opts).await?;

// TTS:
// let tts = engine.tts_provider(&ProviderId::local())?;
// engine.synthesize(…)? / tts.synthesize(…)

engine.shutdown();
```

Rules:

- Pin dependency: `aurum-core = "0.0.20"` or git `tag = "v0.0.20"`.
- Prefer engine-local pools over process-global constructors in long-lived hosts.
- On macOS Metal: clear/shutdown before process exit.
- `local_only` rejects remote providers before network I/O.
- API is **provisional** on 0.0.x — pin versions.

Docs: `docs/library/engine.md`, `docs/library/integration.md`.

## C ABI (`aurum-ffi`)

- Header: `crates/aurum-ffi/include/aurum.h`
- ABI version: **2**
- Surfaces: engine create/destroy, preload, PCM STT, rules cleanup, **local TTS jobs**, capabilities, doctor
- **Not in FFI:** remote providers, mic capture ownership, streaming loop ownership

```bash
cargo build -p aurum-ffi --release
```

See `skills/aurum-embed/` and `docs/library/ffi.md`.
