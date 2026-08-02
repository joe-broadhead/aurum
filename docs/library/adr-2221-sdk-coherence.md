# ADR: SDK coherence for continuous 0.0.x (JOE-2221)

## Status

Accepted for v0.0.22 Wave 1.

## Context

Aurum grew sound pieces in isolation: `AurumEngine`, `ValidatedConfig`,
provider traits, DTOs, and `OpContext`. The public surface still mixed
CLI flat `Config`, transcription-named errors, and a wide root re-export.

## Decision

1. **`AurumError` is the concrete product error**; `TranscriptionError` is a
   one-cycle alias.
2. **`AurumConfig`** is the library direction-oriented view; `ConfigFile`
   stays the file schema; flat `Config` remains for CLI conversion.
3. **`OperationOptions`** is the shared cancel/deadline/progress/request-id
   contract for STT, TTS, and cleanup; maps to `OpContext`.
4. **`TranscriptionRequest` / `SynthesisRequest`** are typed host requests
   with construction-time validation.
5. **`prelude`** is the intentional common import set; advanced modules stay
   explicit.
6. **`AurumEngine` remains the preferred ownership boundary.**

## Consequences

* Downstream hosts migrate with a thin rename and optional prelude import.
* No second authoritative runtime graph: file → validated → engine once.
* C ABI and CLI behaviour stay stable for this issue.

## Links

* Migration: `docs/library/migration-0.0.21-0.0.22.md`
* Parent epic: JOE-2215
