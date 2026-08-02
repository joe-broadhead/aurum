# RC compatibility freeze inventory (JOE-1896) — v0.0.21 programme

**Document version:** 1.1  
**Freeze anchor package version:** `0.0.16` (inventory first introduced); current product tip **0.0.20**  
**Parent epic:** [JOE-1655](https://linear.app/joe-broadhead/issue/JOE-1655) (retargeted to **v0.0.21**)

This inventory names the **public surfaces frozen for a production RC interval**
toward the **v0.0.21** assurance cut (formerly labelled “1.0 RC”). On the continuous
0.0.x line, additive patches remain allowed; **removing or renaming** a frozen
item during RC **resets the freeze clock** (see § Breaking-change policy).

See also [compatibility.md](../development/compatibility.md) for 0.0.x classes.

## Frozen surfaces (RC)

### CLI / exit categories

| Item | Freeze rule | Evidence |
|------|-------------|----------|
| Primary commands: `transcribe`, `tts`, `doctor`, `cache`, `models`, `cleanup` (as shipped) | No remove/rename without freeze reset | `docs/reference/cli-help.md`, CLI help tests |
| Exit codes from `ErrorCategory::exit_code` mapping | Numeric categories 1–7 stable | `crates/aurum-core/src/error.rs` + unit tests |
| Offline doctor / cache verify | Must remain fail-closed, no surprise network | `doctor` / cache tests |

### Config

| Item | Freeze rule | Evidence |
|------|-------------|----------|
| `Config` load path + `validate` / `ValidatedConfig` | No silent renames of keys required for local STT/TTS | config unit tests |
| Secret redaction in diagnostics | Always-on | `secret` + config diagnostic tests |
| OpenRouter keys only via env/config (never logs) | Stable | credential hygiene docs |

### JSON DTO schemas

| DTO | `schema_version` | Freeze rule |
|-----|------------------|-------------|
| `SttResultDto` | **1** | Library + CLI STT JSON (`-o json`); unsupported version → error |
| `TtsMetaDto` | **1** | Library honesty JSON; CLI `--emit-json` should use this DTO (includes `schema_version`) |
| `ErrorDto` | **1** | Library + CLI structured errors: when `-o json` / `--json` / `--emit-json` (or `AURUM_JSON_ERRORS=1`) process exit still uses `ErrorCategory::exit_code` **and** stderr carries a full `ErrorDto` JSON envelope |

Constants: `STT_RESULT_SCHEMA_VERSION`, `TTS_META_SCHEMA_VERSION`, `ERROR_SCHEMA_VERSION` in `aurum-core` dto module.

### C ABI (FFI)

| Item | Value | Freeze rule |
|------|-------|-------------|
| `AURUM_ABI_VERSION` | **2** | Breaking C changes bump version + header |
| `AURUM_ABI_MIN_VERSION` | **2** | Greenfield: equals current ABI (no dual-version lag) |
| `AURUM_SAMPLE_RATE` | **16000** | PCM contract |
| Jobs / cleanup / doctor / capabilities | Present in ABI v2 | Additive status codes preferred |

Evidence: `crates/aurum-ffi/tests/abi_layout.rs`, `include/aurum.h`.

### Cache / manifests

| Item | Freeze rule |
|------|-------------|
| STT pin identity = filename + SHA-256 + exact size | Trusted catalogue only |
| Cache layout under user cache dir | Documented; quarantine on verify failure |
| TTS pack digests / trust modes | `builtin` / `verified` / `local_unverified` semantics |

### Model / voice IDs (defaults)

| Surface | Default / pin policy |
|---------|----------------------|
| Local STT catalogue | Reviewed `ggml-*.bin` pins in `model::pinned_sha256` |
| Default TTS catalogue id | Built-in kitten catalogue (`DEFAULT_TTS_MODEL`) |
| Custom/BYOM ids | Must not shadow built-in catalogue when `tts` feature on |

## Breaking-change policy (RC)

1. **Additive only** during RC: new flags with defaults, new optional DTO fields, new ABI status codes.
2. **Breaking** = remove/rename public Rust/C/CLI/DTO fields, change exit category numbers, change schema_version without dual-read, change default model ids silently.
3. A breaking change **resets RC**: re-cut RC branch, re-run dogfood ([rc-dogfood.md](rc-dogfood.md)), update this inventory’s freeze anchor version.
4. Prefer one deprecation release before removal when practical ([compatibility.md](../development/compatibility.md)).

## Automated freeze checks

```bash
./scripts/rc_freeze_check.sh
```

Runs ABI constant tests, DTO schema_version unit tests, model pin catalogue guard,
and mutation_semantics kill list. Wired into CI as job `rc-freeze-check`.

## Related

* [rc-dogfood.md](rc-dogfood.md) · [rc-rollback.md](rc-rollback.md) · [support-policy.md](support-policy.md)
* [release-gate.md](release-gate.md) · [compatibility.md](../development/compatibility.md)
