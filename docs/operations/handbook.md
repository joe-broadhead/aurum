# Operator & integrator handbook (JOE-1639)

Quick map of production docs (read this first):

| Topic | Doc |
|-------|-----|
| Install / quickstart | [Getting started](../getting-started/installation.md) |
| CLI | [CLI](../getting-started/cli.md) |
| Config | [Configuration](../guide/configuration.md) |
| TTS / BYOM | [TTS](../guide/tts.md) |
| FFI / jobs | [FFI](../library/ffi.md) |
| Doctor | `aurum doctor` / `aurum doctor --json` |
| Threat model | [Threat model](threat-model.md) |
| Hardening | [Hardening](hardening.md) |
| Release gate | [Release gate](release-gate.md) |
| RC freeze inventory | [RC freeze](rc-freeze.md) |
| RC dogfood | [RC dogfood](rc-dogfood.md) |
| RC rollback | [RC rollback](rc-rollback.md) |
| Support / security-fix policy | [Support policy](support-policy.md) |
| RC exit report | [RC exit](rc-exit.md) |
| External security review brief | [External review brief](external-review-brief.md) |
| Supply chain | [Supply chain](../development/supply-chain.md) |
| Compatibility | [Compatibility](../development/compatibility.md) |
| Migration 0.0.3 | [Migration](../development/migration-0.0.3.md) |
| Architecture | [Architecture](../development/architecture.md) |
| ADRs | [Kokoro ADR-001](../development/adr-001-kokoro-tts-adapter.md) |
| Security reporting | [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md) |

## Diagnose

```bash
aurum doctor
aurum cache status
aurum tts adapters
```

## Upgrade

1. Read CHANGELOG for the target version.
2. Run migration notes if present.
3. `aurum doctor` after install.
4. Verify release checksums when installing binaries.

## Recover

| Symptom | First checks |
|---------|----------------|
| Model missing | `aurum cache status`; drop `local_only` once to download |
| Corrupt cache | `aurum cache verify` / repair |
| FFmpeg errors | `aurum doctor` ffmpeg check |
| TTS pack failure | `aurum tts verify <pack>` |
| FFI busy on shutdown | wait/cancel jobs; `aurum_shutdown_ex` |
