# AurumEngine (library hosts)

`AurumEngine` is the preferred **owned** entry point for Rust library hosts
(JOE-1654 / JOE-1782). It holds a validated configuration, an engine-local
resource governor, and engine-local metrics.

```rust
use aurum_core::AurumEngine;

let engine = AurumEngine::load()?;
let report = engine.doctor();
let bundle = engine.support_bundle(None);
// High-level STT still uses providers today; engine owns config/governor/metrics.
engine.shutdown();
```

## What the engine owns

| Component | Scope |
|-----------|--------|
| `ValidatedConfig` | Engine |
| `ResourceGovernor` | Engine-local `Arc` (not shared across engines) |
| `Metrics` | Engine-local `Arc` |
| Lifecycle `closed` flag | Engine |

## Residual process-global state (honest)

Local whisper contexts and TTS session pools remain **process-global** in the
current release line. Multiple engines share those caches. Call
`aurum_core::providers::local::clear_context_cache()` before process exit when
using Metal. Per-engine model isolation is a follow-up under JOE-1654.

## ValidatedConfig

```rust
use aurum_core::{Config, ValidatedConfig};

let cfg = Config::load()?;
let validated = ValidatedConfig::try_from_config(cfg)?;
let engine = AurumEngine::new(validated);
```

Invalid provider/output/TTS ceilings fail closed at construction.

## Segment construction

```rust
use aurum_core::Segment;

let seg = Segment::try_new(0.0, 1.2, "hello")?; // rejects NaN / inverted
```

Deserialized segments are untrusted DTOs — call `validate()` before use.
