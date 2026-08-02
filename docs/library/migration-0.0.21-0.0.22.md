# Migration: v0.0.21 → v0.0.22 (JOE-2221)

Aurum remains on the continuous **0.0.x** line. This guide covers the SDK shape
introduced for library hosts. CLI flags and the C ABI are unchanged unless a
changelog entry says otherwise.

## Error type

| v0.0.21 | v0.0.22 |
|---------|---------|
| `TranscriptionError` (concrete) | `AurumError` (concrete) |
| `AurumError` type alias | `TranscriptionError` type alias (one cycle) |

```rust
// Preferred
use aurum_core::{AurumError, Result};

// Still compiles for one release
use aurum_core::TranscriptionError;
```

Provider-scoped credentials:

```rust
// Preferred for new call sites
UserError::MissingProviderCredential { provider: "openai".into() }
// Historical OpenRouter-only variant remains
UserError::MissingApiKey
```

## Configuration

| Concern | Type |
|---------|------|
| On-disk TOML | `ConfigFile` |
| CLI flat runtime bag | `Config` / `ValidatedConfig` |
| Library direction-oriented | `AurumConfig` |

```rust
use aurum_core::prelude::*;

// Recommended host path
let ac = AurumConfig::load()?;
let engine = AurumEngine::new(ac.into_validated());

// Compatibility: still valid
let engine = AurumEngine::load()?;
```

`ConfigFile` is not a second authoritative runtime graph. Convert once into
validated config / `AurumConfig`.

## Operation control

STT, TTS, and cleanup share [`OperationOptions`]:

```rust
use aurum_core::{OperationOptions, TranscriptionRequest};
use std::time::Duration;

let op = OperationOptions::new().with_timeout_from_now(Duration::from_secs(60));
let req = TranscriptionRequest::new("base")
    .language("en")
    .timestamps(false)
    .operation(op);
req.validate()?;
// Engine methods continue to accept TranscriptionOptions today; request types
// are the typed host contract and convert into OpContext via into_op_context().
let ctx = req.operation.into_op_context();
```

## Prelude

```rust
use aurum_core::prelude::*;
// AurumEngine, AurumConfig, AurumError, OperationOptions, ProviderId, …
```

Root `pub use` remains broad for existing crates; new code should import from
`prelude` and explicit advanced modules (registry builders, pack parsers,
process-global pools).

## Batch manifests

v1 batch manifests are rejected. See `docs/guide/batch.md`.

## Feature gates

STT-only hosts:

```toml
aurum-core = { version = "0.0.22", default-features = false }
```

TTS types (`SynthesisRequest`, `TtsConfig`) require the `tts` feature.

## Non-goals of this migration

* Declaring 1.0 stability  
* Changing speech model quality  
* Exposing remote providers on the C ABI  
