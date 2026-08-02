# AurumEngine (library hosts)

`AurumEngine` is the preferred **owned** entry point for Rust library hosts
(JOE-1654 / JOE-1782 / JOE-1784 / JOE-1787 / **JOE-1938** / **JOE-2221**).

For typed configuration and request contracts see
[`migration-0.0.21-0.0.22.md`](migration-0.0.21-0.0.22.md) and
`aurum_core::prelude`.

It holds:

* validated configuration
* engine-local resource governor
* engine-local metrics
* **engine-local STT context pool** and (with `tts`) **TTS session pool**
* immutable **provider registry** (builtin factories)

```rust
use aurum_core::{AurumEngine, ProviderId, ProviderResolveOptions, TranscriptionOptions};

let engine = AurumEngine::load()?;
let report = engine.doctor();
let bundle = engine.support_bundle(None);

// Registry resolution (same path as CLI)
let stt = engine.stt_provider(&ProviderId::local())?;
// let remote = engine.stt_provider(&ProviderId::openrouter())?; // needs key

// High-level STT routes via config `provider` (default local)
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
| `ProviderRegistry` | Engine-local `Arc` (JOE-1938) |
| Lifecycle `closed` flag | Engine |

## Provider resolution (JOE-1938)

| API | Behaviour |
|-----|-----------|
| `engine.registry()` | Builtin local STT/TTS + OpenRouter STT factories |
| `engine.stt_provider(id)` / `tts_provider(id)` | Build via factory + single-id secret scope |
| `engine.transcribe` / `synthesize` | Use config `provider` / `tts_provider` |
| `local_whisper` / `local_tts` | Convenience local paths (still valid) |

Build context never receives a multi-vendor secret bag — only
`config.provider_secret(id)`. CLI `aurum` / `aurum tts` / `aurum batch` use the
same APIs (no growing `match` on provider names).

## Isolation (JOE-1784)

Independent engines do **not** share whisper/TTS residency:

```rust
let a = AurumEngine::load()?;
let b = AurumEngine::load()?;
// a.stt_pool() and b.stt_pool() are distinct Arcs
a.shutdown(); // does not clear b's models
```

Default `LocalWhisperProvider::new` / `LocalTtsProvider::new` use
**process-global** pools (CLI path). Library hosts should prefer:

* `engine.stt_provider(&ProviderId::local())` / `engine.tts_provider(...)`
* `engine.local_whisper()` / `engine.local_tts()` (local convenience)
* `engine.clear_model_caches()` or `engine.shutdown()`

Process-global cleanup for CLI/Metal exit: `aurum_core::clear_context_cache()`.

## ValidatedConfig

```rust
use aurum_core::{Config, ValidatedConfig, AurumEngine};

let cfg = Config::load()?;
let validated = ValidatedConfig::try_from_config(cfg)?;
let engine = AurumEngine::new(validated);
```

## Secrets

`Config.openrouter_api_key` is `Option<SecretString>`. Pass the secret through
`Config::provider_secret(&ProviderId::openrouter())` or clone
`openrouter_api_key` into providers/`OpenRouterCleanup` — never convert to a
plaintext `String` for long-lived storage.

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
