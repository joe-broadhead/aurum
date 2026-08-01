# AurumEngine (library hosts)

`AurumEngine` is the preferred **owned** entry point for Rust library hosts
(JOE-1654 / JOE-1782 / JOE-1784 / JOE-1787). It holds:

* validated configuration
* engine-local resource governor
* engine-local metrics
* **engine-local STT context pool** and (with `tts`) **TTS session pool**

```rust
use aurum_core::{AurumEngine, TranscriptionOptions};

let engine = AurumEngine::load()?;
let report = engine.doctor();
let bundle = engine.support_bundle(None);

// High-level STT (uses this engine's pool + governor + metrics)
let opts = TranscriptionOptions {
    model: "tiny-q5_1".into(),
    language: "en".into(),
    timestamps: false,
    cancel: None,
};
// let result = engine.transcribe_pcm(&samples, &opts).await?;

engine.shutdown(); // closes + clears idle model residency in *this* engine
```

## What the engine owns

| Component | Scope |
|-----------|--------|
| `ValidatedConfig` | Engine |
| `ResourceGovernor` | Engine-local `Arc` |
| `Metrics` | Engine-local `Arc` |
| `SttContextPool` | Engine-local `Arc` (JOE-1784) |
| `TtsSessionPool` | Engine-local `Arc` when feature `tts` |
| Lifecycle `closed` flag | Engine |

## Isolation (JOE-1784)

Independent engines do **not** share whisper/TTS residency:

```rust
let a = AurumEngine::load()?;
let b = AurumEngine::load()?;
// a.stt_pool() and b.stt_pool() are distinct Arcs
a.shutdown(); // does not clear b's models
```

Default `LocalWhisperProvider::new` / `LocalTtsProvider::new` still use
**process-global** pools for CLI compatibility. Prefer:

* `engine.local_whisper()` / `engine.local_tts()`
* `LocalWhisperProvider::with_runtime(dir, pool, governor)`
* `engine.clear_model_caches()` or `engine.shutdown()`

Process-global cleanup (legacy CLI/Metal exit):
`aurum_core::clear_context_cache()`.

## ValidatedConfig

```rust
use aurum_core::{Config, ValidatedConfig, AurumEngine};

let cfg = Config::load()?;
let validated = ValidatedConfig::try_from_config(cfg)?;
let engine = AurumEngine::new(validated);
```

## Secrets

`Config.openrouter_api_key` is `Option<SecretString>`. Use
`Config::openrouter_api_key_exposed()` only when constructing a remote client.

## Segment / result construction (JOE-1786)

`Segment` fields are **private**. Use accessors (`start()`, `end()`, `text()`)
and construct with `try_new` (fail closed) or `from_parts_unchecked` only on
trusted paths.

```rust
use aurum_core::{Segment, TranscriptionResult, SampleRateHz, ModelId};

let _rate = SampleRateHz::whisper();
let _model = ModelId::try_new("tiny-q5_1")?;
let seg = Segment::try_new(0.0, 1.2, "hello")?; // rejects NaN / inverted
let result = TranscriptionResult::try_local(
    "hello".into(),
    vec![seg],
    Some("en".into()),
    "tiny-q5_1".into(),
    1.2,
)?;
```

Deserialized segments are untrusted DTOs — call `validate()` /
`validate_segments()` before use.
