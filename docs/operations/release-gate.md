# Release gate (JOE-1640 / JOE-1578)

Aurum **v0.0.x** is pre-1.0. This gate defines what must be true before tagging any
release, and what additional freezes apply before a future **1.0**.

## Automated gate (every tag)

```bash
./scripts/release_gate.sh
```

Enforced in CI:

| Gate | Workflow |
|------|----------|
| fmt / clippy / tests | `ci.yml` |
| MSRV | `ci.yml` msrv |
| STT-only build + adversarial tests | `ci.yml` stt-only |
| Fail-closed security tool pins | `scripts/check_security_tool_pins.sh` (JOE-1715) |
| crates.io credential not chat-disclosed | operator rotation checklist in [credential-hygiene](credential-hygiene.md) |
| Docs strict | `ci.yml` Docs |
| Integration smoke | `ci.yml` integration-local |
| RustSec + cargo-deny | `ci.yml` security |
| Version sync | `scripts/version_check.sh` |
| Tag ≡ VERSION + CHANGELOG | `release.yml` validate |
| Platform binaries + SHA256SUMS + formal SBOM | `release.yml` build/publish |
| CycloneDX + SPDX SBOM required | `scripts/generate_sbom.sh` + `verify_release_assets.sh` (JOE-1859) |
| PROVENANCE.json + verify | `scripts/generate_provenance.sh` (JOE-1860) |
| Cosign keyless `SHA256SUMS.bundle` (required) | `release.yml` publish + `verify_release_assets.sh` (JOE-1882) |
| Short cargo-fuzz smoke | `ci.yml` fuzz-smoke (JOE-1861 / JOE-1884) |
| Scheduled long fuzz campaigns | `fuzz-campaign.yml` (JOE-1884) |
| Tier A clean-install matrix | `ci.yml` clean-install (JOE-1883) |
| Two-builder variance report | `ci.yml` repro-smoke / `release.yml` repro-compare (JOE-1885) |

A release **cannot** publish if validate/test/security steps fail (fail-closed).

See also [provenance.md](provenance.md), [platform-support.md](platform-support.md), [fuzzing.md](fuzzing.md), [reproducibility.md](reproducibility.md).

## Compatibility freeze (toward 1.0)

Before declaring 1.0, freeze and test:

- Supported CLI commands/flags/exit categories
- Config schema + precedence
- JSON DTO schema versions (`SttResultDto`, `TtsMetaDto`, `ErrorDto`)
- C ABI v2 exports and status codes
- Default model/voice IDs and pack manifest schema
- Cache layout for STT/TTS

Documented provisional surfaces remain experimental until listed in
[compatibility.md](../development/compatibility.md).

## RC dogfood (1.0)

1. Cut `release/x.y.z` from green master.
2. Run RC for ≥ N days with real workflows (local STT, cleanup, TTS, doctor).
3. Record regressions in CHANGELOG; no silent contract breaks.
4. Security issues go through [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md).

## Rollback

1. Yank crates.io versions only if safety-critical (prefer yank + advisory).
2. GitHub Release: publish a fixed tag `vX.Y.Z+1` — do not rewrite tags.
3. Document migration in `docs/development/migration-*.md`.

## Support matrix (tiers)

Full detail: [platform-support.md](platform-support.md).

| Tier | Platforms | Artifacts |
|------|-----------|-----------|
| **A — binary** | macOS arm64, Linux x86_64 gnu, Windows x86_64 MSVC | GitHub Release CLI |
| **B — source** | macOS x86_64, other Linux | `cargo install aurum-stt --locked` |
| **C — experimental** | other targets | best-effort `cargo build` |

Clean-install smoke: `./scripts/clean_install_smoke.sh --from-source` (or `--from-release`).

See [release.md](../development/release.md) for the operator flow.
