# Public surface classification (JOE-1612 / JOE-1780 / JOE-1896)

**Boundary:** contracts for the continuous **0.0.x** line. Change stays
**predictable and honest**. For the **v0.0.22 RC freeze inventory** (frozen
surfaces, break-reset policy, automated checks), see
[rc-freeze.md](../operations/rc-freeze.md).

This is a **greenfield** 0.0.x product: there is no dual-path loader for old
config sections, no deprecation lag for removed internal helpers, and no
migration shims for pre-canonical layouts. Prefer delete over leave-for-compat.

## Classification

| Surface | Class | Notes |
|---------|--------|--------|
| `TranscriptionError` / `AurumError`, `ErrorCategory` | **stable-at-0.x** | Category ids frozen; variants may grow under non-exhaustive policy |
| `Config`, `Config::validate`, `effective_diagnostic` | **stable-at-0.x** | Canonical TOML only (`[stt]`, `[cleanup]`, `[tts]`, `[providers.*]`); secrets redacted in Debug/diagnostic |
| `ValidatedConfig`, `AurumEngine` | **stable-at-0.x** | Preferred library entry; owns governor/metrics/STT(+TTS) pools |
| `SttContextPool` / `TtsSessionPool` | **provisional** | Engine-local by default; process-global helpers for CLI |
| `SecretString` | **stable-at-0.x** | Debug/Display never expose payload |
| `Segment` (private fields + accessors) | **stable-at-0.x** | `try_new` / `validate` / getters (JOE-1786) |
| `SampleRateHz` / `FiniteDurationSecs` / `ModelId` | **provisional** | Domain primitives |
| `TranscriptionResult::try_local` / `try_openrouter` | **stable-at-0.x** | Fail-closed builders |
| Process-global STT/TTS pools | **cli default** | Used when constructing providers without an engine; not a second product path |
| `SttResultDto` `schema_version = 1` | **stable-at-0.x** | Unknown future fields: ignore on read when possible; unsupported version → error |
| `ErrorDto` `schema_version = 1` | **stable-at-0.x** | Library + CLI machine JSON error envelope |
| `ProviderCapabilities` `schema_version = 1` | **stable-at-0.x** | Preflight before expensive work; optional remote fields are additive with defaults |
| `PartialSession`, `PcmBuffer`, `ResourceGovernor` | **stable-at-0.x** | Host-facing concurrency / progressive STT |
| C ABI (`AURUM_ABI_VERSION = 2`) | **provisional** | Jobs include STT/cleanup/TTS; additive status codes preferred |
| Internal download, remote client, postprocess | **internal** | May change without notice |
| Experimental adapters / experimental STT models | **feature-gated / experimental** | Not part of the stability claim |

## SemVer (0.0.x continuous)

Aurum iterates as **0.0.x** patch steps. A major `1.0.0` is **not** planned for
the current programme; production-assurance work targets **v0.0.22**.

- **PATCH (0.0.x):** bugfixes, docs, additive diagnostics, non-breaking DTO fields with defaults, evidence packs, provider catalogue refresh
- **Breaking 0.0.x:** removed public items require a CHANGELOG note (no multi-release deprecation lag)

## Unknown JSON fields

Readers of `schema_version = 1` DTOs should ignore unknown fields. Writers must
not emit NaN/Inf. Unsupported `schema_version` values fail clearly.

## Fixtures

- STT JSON: `schema_version` present; see unit tests in `dto` / `output`
- Config: `load_from` / `load_from_required` / redacted diagnostic / `ValidatedConfig`
- Engine: multi-engine metrics isolation tests
- ABI: `aurum-ffi` `abi_layout` tests

## Release checklist

1. Review public surfaces against this document
2. Update classifications when the surface changes
3. CHANGELOG note for any breaking 0.0.x change
4. Run `cargo test` + clippy + FFI abi tests
