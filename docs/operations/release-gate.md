# Release gate (JOE-1640 / JOE-1578)

Aurum ships on a continuous **0.0.x** line. This gate defines what must be true
before tagging any release, and what additional freezes apply for the
**v0.0.22 product-outcomes cut** (JOE-2215 / JOE-2226); prior programme v0.0.21.

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
| Miri pure filters | `ci.yml` miri / `scripts/run_miri.sh` (JOE-1889) |
| ASan + concurrency stress | `ci.yml` sanitizer-stress / `scripts/run_sanitizers.sh` (JOE-1887) |
| Trust-boundary coverage report | `ci.yml` coverage-trust / `scripts/coverage_trust.sh` (JOE-1888) |
| Mutation smoke (scoped) | `ci.yml` mutants-smoke / `scripts/run_mutants.sh` (JOE-1886) |
| Security rehearsal (revoke + disclosure) | `ci.yml` security-rehearsal (JOE-1890 / JOE-1892) |
| Independent release verify (download + cosign) | `release-verify.yml` (JOE-1891) |
| Threat-model control matrix | [threat-model.md](threat-model.md) (JOE-1893) |
| RC freeze inventory check | `ci.yml` rc-freeze-check / `scripts/rc_freeze_check.sh` (JOE-1896) |
| RC dogfood automated subset | `ci.yml` rc-dogfood-smoke / `scripts/rc_dogfood_checklist.sh` (JOE-1897) |
| RC rollback rehearsal | `scripts/rehearse_rc_rollback.sh` (JOE-1895) |
| Support / security-fix policy | [support-policy.md](support-policy.md) (JOE-1898) |
| Downstream consumer gate | `ci.yml` rc-exit-pack / `scripts/rc_downstream_check.sh` (JOE-1903) |
| Native inventory freeze | `native-components.md` via `generate_sbom.sh` (JOE-1902) |
| RC exit report | `scripts/generate_rc_exit_report.sh` (JOE-1904 / JOE-2226) |
| Product acceptance index | [v0022-product-acceptance.md](v0022-product-acceptance.md) (JOE-2226) |
| Product contracts drift | `scripts/check_product_contracts.sh` (JOE-2224) |
| Provider evidence freshness | `scripts/check_provider_evidence.sh` (JOE-2223) |
| Native SDK package + qualify | `release.yml` sdk-build / `package_native_sdk.sh` (JOE-2225) |
| External review brief | [external-review-brief.md](external-review-brief.md) (JOE-1901) |
| Provider platform qualification | [provider-qualification.md](provider-qualification.md) (JOE-1943 / JOE-2223) |
| Provider matrix / support tiers | [provider-matrix.md](../guide/provider-matrix.md) |

A release **cannot** publish if validate/test/security steps fail (fail-closed).

### Provider-platform extras (JOE-1943)

Before tagging when remote providers changed:

1. Local-only smoke with **no** real cloud keys (STT + TTS).
2. `check_builtin_conformance` + qualification isolation tests green.
3. Support tiers and privacy wording reviewed (see provider-qualification).
4. Optional protected credentialed smokes for tiers marked **supported**.

See also [provenance.md](provenance.md), [platform-support.md](platform-support.md), [fuzzing.md](fuzzing.md), [reproducibility.md](reproducibility.md), [qe-depth.md](qe-depth.md), [model-revocation.md](model-revocation.md), [disclosure-tabletop.md](disclosure-tabletop.md), [rc-freeze.md](rc-freeze.md), [rc-dogfood.md](rc-dogfood.md), [rc-rollback.md](rc-rollback.md), [rc-exit.md](rc-exit.md).

## Compatibility freeze (toward v0.0.22)

Full inventory: **[rc-freeze.md](rc-freeze.md)** (JOE-1896).

Before declaring the **v0.0.22** product cut, freeze and test:

- Supported CLI commands/flags/exit categories
- Config schema + precedence
- JSON DTO schema versions (`SttResultDto`, `TtsMetaDto`, `ErrorDto`)
- C ABI v2 exports and status codes
- Default model/voice IDs and pack manifest schema
- Cache layout for STT/TTS

Documented provisional surfaces remain experimental until listed in
[compatibility.md](../development/compatibility.md). Breaking a frozen surface
during RC **resets the freeze**.

## RC dogfood (v0.0.22)

Full checklist: **[rc-dogfood.md](rc-dogfood.md)** (JOE-1897).

1. Cut `release/x.y.z` from green master.
2. Run RC for ≥ N days with real workflows (local STT, cleanup, TTS, doctor).
3. Record regressions in CHANGELOG; no silent contract breaks.
4. Security issues go through [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md).
5. Fill per-platform evidence templates under `dist/rc-dogfood/`.

## Rollback

Full rehearsal: **[rc-rollback.md](rc-rollback.md)** (JOE-1895).

1. Yank crates.io versions only if safety-critical (prefer yank + advisory).
2. GitHub Release: publish a fixed tag `vX.Y.Z+1` — do not rewrite tags.
3. Document migration in `docs/development/migration-*.md`.
4. Human sign-off required for the **v0.0.22** product cut (automation cannot self-declare).

## Support matrix (tiers)

Full detail: [platform-support.md](platform-support.md).

| Tier | Platforms | Artifacts |
|------|-----------|-----------|
| **A — binary** | macOS arm64, Linux x86_64 gnu, Windows x86_64 MSVC | GitHub Release CLI |
| **B — source** | macOS x86_64, other Linux | `cargo install aurum-stt --locked` |
| **C — experimental** | other targets | best-effort `cargo build` |

Clean-install smoke: `./scripts/clean_install_smoke.sh --from-source` (or `--from-release`).

See [release.md](../development/release.md) for the operator flow.
