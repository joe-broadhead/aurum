# v0.0.22 product acceptance evidence index (JOE-2226)

**Date (UTC):** 2026-08-03  
**Programme:** [JOE-2215](https://linear.app/joe-broadhead/issue/JOE-2215) — World-Class Product Outcomes & SDK Coherence  
**Gate issue:** [JOE-2226](https://linear.app/joe-broadhead/issue/JOE-2226)  
**Candidate product version:** **0.0.22**  
**Proposed tag:** `v0.0.22`  
**Owner:** Joseph Broadhead  

This document is the redacted, reconstructible **evidence index** for the
v0.0.22 exact-candidate gate. Automation prepares evidence. **A human records
GO/NO-GO before any tag or publication.** Agents must not self-approve.

## Candidate identity (fill at freeze / tag)

| Field | Value |
|-------|--------|
| Full source commit (40-char) | _record at freeze_ |
| Workspace `VERSION` | `0.0.22` |
| Proposed tag | `v0.0.22` |
| Rust edition / MSRV | edition 2021 / `rust-version` 1.89 |
| Default features | workspace default (STT + TTS + cleanup) |
| ABI min / current | **2** / **2** |
| STT JSON DTO schema | **2** |
| TTS meta / Error DTO | **1** / **1** |
| Batch manifest schema | **2** (v1 rejected) |
| Product-contract schema | **1** (regenerated at tip) |
| STT observatory evidence | `0.0.22-observatory-v1` |
| TTS listening evidence | `0.0.22-tts-listening-v1` |
| Perf evidence | `0.0.22-perf-v1` |
| Provider evidence max age | 30 days for `supported` routes |

Record CI run IDs on the **immutable** freeze/tag commit only. Do not
substitute evidence from later mutable master commits.

## Entry criteria

| Criterion | Status |
|-----------|--------|
| JOE-2216 … JOE-2225 complete against testable AC | **Met** (Waves 1–3 PRs #90–#92) |
| No open Critical/High security or data-integrity findings | **Met** (prior freeze + post-provider disposition) |
| Medium residuals documented with owner/claim impact | **Met** (see residuals below) |
| Generated product surfaces current | **Met** — `./scripts/check_product_contracts.sh` |
| Support-tier records fresh under JOE-2223 | **Met for local** required routes; remotes **experimental** |
| Quality/perf baselines committed | **Met** under `evals/reports/` + budgets |
| Repository clean of unintended changes at freeze | Operator check at tag |

A child marked Done without reports does **not** satisfy entry. Status alone is
not evidence.

## Child issue map

| Issue | Outcome |
|-------|---------|
| JOE-2216 STT observatory | Done — corpus/schema/budgets/reports |
| JOE-2217 TTS listening | Done — protocol + objective samples |
| JOE-2218 Performance | Done — named-hardware reports |
| JOE-2219 Long-form STT | Done — boundary windows + provenance |
| JOE-2220 Batch integrity | Done — manifest v2 |
| JOE-2221 SDK coherence | Done — AurumError/AurumConfig/OperationOptions |
| JOE-2222 Observability | Done — OpEvent privacy-safe path |
| JOE-2223 Provider evidence | Done — gate + local evidence; remotes experimental |
| JOE-2224 Product contracts | Done — generate/check |
| JOE-2225 Native SDK | Done — package/qualify tooling + release matrix |
| JOE-2226 Release gate | **This document + human GO** |

## Machine-checkable gates (run on candidate)

```bash
./scripts/version_check.sh
./scripts/check_product_contracts.sh
./scripts/check_provider_evidence.sh
./scripts/rc_freeze_check.sh
./scripts/generate_rc_exit_report.sh dist/rc-exit
./scripts/release_gate.sh   # full local fail-closed gate; no tag
```

CI on the freeze PR/commit must keep required jobs green (lint, matrix tests,
MSRV, STT-only, security, docs, fuzz/Miri/sanitizer/mutants, clean-install,
rc freeze/exit/dogfood, integration, repro). Product failure → new candidate.

| Area | Artifact / command | Expectation |
|------|-------------------|-------------|
| Version sync | `version_check.sh` | pass |
| Product contracts | `check_product_contracts.sh` | pass |
| Provider evidence | `check_provider_evidence.sh` | pass (local required) |
| Freeze inventory | `rc_freeze_check.sh` | pass |
| RC exit pack | `generate_rc_exit_report.sh` | pass machine rows |
| Release gate | `release_gate.sh` | pass (no publish) |
| STT budgets | `evals/reports/stt/` + compare | within committed budgets |
| TTS listening | `evals/reports/listening/` | protocol samples present |
| Perf | `evals/reports/perf/` | within committed budgets |
| Native SDK package | `package_native_sdk.sh` | archive + manifest digests |
| Native SDK qualify | `qualify_native_sdk_bundle.sh` | bundle-only link path |

## Product golden workflows (operator / CI)

### Local CLI

- Install verified Tier A CLI binary from release assets (post-tag) or
  `cargo install --path crates/aurum --locked` on candidate
- `aurum doctor`, cache verify, model recommendation
- Short local STT → txt/json/srt; long-form where applicable
- Rules cleanup; Kitten TTS; optional Kokoro
- Batch resume: exact resume, interrupted recovery, changed-source reject
- Machine JSON envelopes; support-bundle canary scan

### Rust library

- Recommended engine STT/TTS examples; STT-only feature build
- Operation deadline/cancel/progress; metrics snapshot without payloads

### Native SDK

- Download each Tier A `aurum-sdk-*` archive from the release
- Verify checksums/manifest; C11 + C++17 clean link from **bundle only**
- Capability/version/doctor/rules/job lifecycle; remotes unavailable

### Remote providers

- Routes labelled **supported** must have fresh protected evidence
- **Current product claim:** only **local** STT/TTS routes are `supported`
  for release; OpenRouter/OpenAI/ElevenLabs/xAI remain **experimental**
- Local-only canary with synthetic keys present → zero remote requests

## Release asset rehearsal

Produce **without** publication until human GO:

| Asset class | Platforms |
|-------------|-----------|
| CLI binaries | macOS arm64, Linux x86_64, Windows x86_64 |
| Native SDK archives | same Tier A matrix via `package_native_sdk.sh` |
| SBOM / inventory | CycloneDX + SPDX + package inventory |
| `PROVENANCE.json` / `.txt` | tag commit identity |
| `SHA256SUMS` + cosign bundle | keyless OIDC |
| This evidence index | linked from JOE-2215 / JOE-2226 |

Independent verify: `./scripts/verify_release_assets.sh <dir>` with expected tag
and cosign identity env vars (see [provenance.md](provenance.md)).

## Honest residuals (accepted for v0.0.22)

| Residual | Impact | Owner |
|----------|--------|-------|
| Remote providers experimental until live protected smoke + human promotion PR | Dry-run schema/canary in CI (JOE-2229); do not claim remote `supported` without evidence | Maintainer |
| crates.io package provenance parity / multi-party SLSA L3 | Not claimed; GitHub cosign + PROVENANCE + **SLSA build attestations** required (JOE-2230) | Supply chain residual |
| Full whisper/ORT under ASan/UBSan; macOS/Windows sanitizers | Pure-filter UBSan on Linux CI (JOE-2228); native paths via integration | QE residual |
| Production STT pack ≥60 min multi-speaker real speech | Operator path + recipe/dry-run coverage (JOE-2231); core + metal reports in tree | Product residual |
| Multi-tenant in-process isolation | Explicitly unsupported | Security residual |
| Stable 1.0 Rust API | Not claimed; continuous 0.0.x | Product residual |
| Human multi-day dogfood calendar | Represented by Tier A clean-install + dogfood smoke + human sign-off | RC process residual |

A baseline must not be edited in the release PR solely to convert a failure into
a pass.

## Human sign-off (required — leave blank in automation)

| Field | Value |
|-------|--------|
| Candidate release tag | `v0.0.22` |
| Freeze commit reviewed | |
| STT quality / profile recommendations accepted | |
| TTS listening outcome accepted | |
| Performance budgets accepted | |
| Provider support tiers accepted (local supported; remotes experimental) | |
| API migration + native SDK usability accepted | |
| Security/privacy residuals accepted | |
| **Approver name** | |
| **Approve v0.0.22 tag & publication?** | yes / no |
| Date (UTC) | |

Automation, an engineering agent, or Linear status **cannot** self-approve.

## Post-publication checklist

1. Download public assets from a clean environment  
2. Verify tag/commit/version identity + `verify_release_assets.sh`  
3. Minimal CLI + native SDK smoke  
4. Docs/installer point at `v0.0.22`  
5. Optional crates.io publish (manual, ordered `aurum-core` → `aurum-stt`)  
6. Keep JOE-2226 open until these pass; then close JOE-2226 and JOE-2215  

## Related

* [release-gate.md](release-gate.md) · [rc-exit.md](rc-exit.md) · [rc-freeze.md](rc-freeze.md)  
* [provider-qualification.md](provider-qualification.md) · [stt-observatory.md](stt-observatory.md)  
* [../library/native-sdk.md](../library/native-sdk.md) · [../library/migration-0.0.21-0.0.22.md](../library/migration-0.0.21-0.0.22.md)  
* [v0021-residual-closeout.md](v0021-residual-closeout.md) (prior programme)
