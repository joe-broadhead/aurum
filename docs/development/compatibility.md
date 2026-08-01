# Compatibility & deprecation (JOE-1612 / JOE-1780 / JOE-1896)

**Boundary:** contracts for the **0.0.x** line. Pre-1.0 change stays
**predictable and honest**. For the **1.0 RC freeze inventory** (frozen
surfaces, break-reset policy, automated checks), see
[rc-freeze.md](../operations/rc-freeze.md).

## Classification

| Surface | Class | Notes |
|---------|--------|--------|
| `TranscriptionError` / `AurumError`, `ErrorCategory` | **stable-at-0.x** | Category ids frozen; variants may grow under non-exhaustive policy |
| `Config`, `Config::validate`, `effective_diagnostic` | **stable-at-0.x** | Secrets always redacted in Debug/diagnostic |
| `ValidatedConfig`, `AurumEngine` | **provisional → stable-at-0.x** | Preferred library entry; owns governor/metrics/STT(+TTS) pools |
| `SttContextPool` / `TtsSessionPool` | **provisional** | Engine-local by default; process-global helpers for CLI |
| `SecretString` | **stable-at-0.x** | Debug/Display never expose payload |
| `Segment` (private fields + accessors) | **stable-at-0.x** | `try_new` / `validate` / getters (JOE-1786) |
| `SampleRateHz` / `FiniteDurationSecs` / `ModelId` | **provisional** | Domain primitives |
| `TranscriptionResult::try_local` / `try_openrouter` | **stable-at-0.x** | Fail-closed builders |
| Process-global STT/TTS pools | **legacy shared default** | Used only when constructing providers without an engine |
| `SttResultDto` `schema_version = 1` | **stable-at-0.x** | Unknown future fields: ignore on read when possible; unsupported version → error |
| `ProviderCapabilities` `schema_version = 1` | **stable-at-0.x** | Preflight before expensive work; optional remote TTS/STT fields (JOE-1936) are additive with defaults |
| `PartialSession`, `PcmBuffer`, `ResourceGovernor` | **stable-at-0.x** | Host-facing concurrency / progressive STT |
| C ABI (`AURUM_ABI_VERSION = 2`) | **provisional** | Jobs include STT/cleanup/TTS; additive status codes preferred |
| Process-global whisper/TTS caches | **legacy shared default** | Isolated when using `AurumEngine`; residual only for non-engine paths |
| Internal `postprocess`, download, remote client details | **internal** | May change without notice |
| Experimental adapters / experimental STT models | **feature-gated / experimental** | Not part of the stability claim |

## SemVer (pre-1.0 / 0.0.x)

Aurum currently iterates as **0.0.x** patch steps until a deliberate major step.

- **PATCH (0.0.x):** bugfixes, docs, additive diagnostics, non-breaking DTO fields with defaults, evidence packs
- **Breaking 0.x:** removed public items require a migration note and at least one release of deprecation when practical

## Deprecation window

Prefer one **release** of `#[deprecated]` (or docs-only deprecation) before
removal of public Rust APIs. C ABI breaks require `AURUM_ABI_VERSION` bump and
header notes.

## Unknown JSON fields

Readers of `schema_version = 1` DTOs should ignore unknown fields. Writers must
not emit NaN/Inf. Unsupported `schema_version` values fail clearly.

## Compatibility fixtures

- STT JSON: `schema_version` present; see unit tests in `dto` / `output`
- Config: `load_from` / `load_from_required` / redacted diagnostic / `ValidatedConfig`
- Engine: multi-engine metrics isolation tests
- ABI: `aurum-ffi` `abi_layout` tests

## Release checklist

1. Compatibility review of public surfaces
2. Update this document if classifications change
3. CHANGELOG migration note for any breaking 0.x change
4. Run `cargo test` + clippy + FFI abi tests
