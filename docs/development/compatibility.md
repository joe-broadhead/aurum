# Compatibility & deprecation (JOE-1612)

**Boundary:** contracts frozen for the **0.0.3 / approaching 0.1** line. This is
not a 1.0 freeze — it makes pre-1.0 change **predictable and honest**.

## Classification

| Surface | Class | Notes |
|---------|--------|--------|
| `TranscriptionError` / `AurumError`, `ErrorCategory` | **stable-at-0.1** | Category ids frozen; variants may grow under non-exhaustive policy |
| `Config`, `Config::validate`, `effective_diagnostic` | **stable-at-0.1** | Secrets always redacted in Debug/diagnostic |
| `SttResultDto` `schema_version = 1` | **stable-at-0.1** | Unknown future fields: ignore on read when possible; unsupported version → error |
| `ProviderCapabilities` `schema_version = 1` | **stable-at-0.1** | Preflight before expensive work |
| `PartialSession`, `PcmBuffer`, `ResourceGovernor` | **stable-at-0.1** | Host-facing concurrency / progressive STT |
| C ABI (`AURUM_ABI_VERSION = 1`) | **compatibility** | Additive status codes only; FFI v2 is a later epic |
| Internal `postprocess`, download, remote client details | **internal** | May change without notice |
| Experimental adapters / unshipped TTS models | **feature-gated** | Not part of the stability claim |

## SemVer (pre-1.0)

- **PATCH:** bugfixes, docs, additive diagnostics, non-breaking DTO fields with defaults
- **MINOR (0.x):** new modules, new error variants, new capability fields; deprecations
- **Breaking 0.x:** removed public items require a migration note and at least one
  release of deprecation when practical

## Deprecation window

Prefer one **minor** release of `#[deprecated]` (or docs-only deprecation) before
removal of public Rust APIs. C ABI breaks require `AURUM_ABI_VERSION` bump and
header notes.

## Unknown JSON fields

Readers of `schema_version = 1` DTOs should ignore unknown fields. Writers must
not emit NaN/Inf. Unsupported `schema_version` values fail clearly.

## Compatibility fixtures

- STT JSON: `schema_version` present; see unit tests in `dto` / `output`
- Config: `load_from` / `load_from_required` / redacted diagnostic
- ABI: `aurum-ffi` `abi_layout` tests

## Release checklist

1. Compatibility review of public surfaces
2. Update this document if classifications change
3. CHANGELOG migration note for any breaking 0.x change
4. Run `cargo test` + clippy + FFI abi tests
