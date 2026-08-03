# v0.0.23 product acceptance (correctness patch)

**Date (UTC):** 2026-08-03  
**Candidate product version:** **0.0.23**  
**Proposed tag:** `v0.0.23`  
**Owner:** Joseph Broadhead  

This document is the redacted **evidence and GO record** for the v0.0.23
correctness & integration patch after the independent v0.0.22 audit.

## Human GO

```text
Human approval personally recorded by Joseph Broadhead at 2026-08-03
(session instruction: “do it” — freeze, tag, publish GitHub release and crates.io).
```

Agents did **not** self-approve the tag. Publication proceeds under this
maintainer GO.

## Candidate identity

| Field | Value |
|-------|--------|
| Workspace `VERSION` | `0.0.23` |
| Proposed tag | `v0.0.23` |
| Parent published tip | `v0.0.22` (`f2c1565…`) |
| Freeze content | P0 correctness (#95) + P1 batch/SDK/obs (#96) + OpContext + native SDK (#97) on master tip at freeze |

Record the **exact tag commit** (40-char) after merge/tag in post-publication notes.

## What this cut claims

| Claim | Status |
|-------|--------|
| Local / dedicated-ASR SRT works without `--allow-unreliable-timestamps` when provenance is set | **Met** (provider provenance + SRT gate tests) |
| Long-form overlap ownership + silence scan hang fixed | **Met** |
| Batch re-verifies source identity after decode | **Met** |
| Operation fingerprint includes long-form policy + local model digest | **Met** |
| `TranscriptionRequest` / `SynthesisRequest` drive engine execution | **Met** |
| Parent `OpContext` shared across providers / long-form chunks | **Met** |
| Native SDK CMake declares system link deps; qualify exercises CMake + pkg-config | **Met** (macOS qualify run locally; Windows MSVC when `cl` present on GHA release) |

## What this cut does **not** claim

| Residual | Note |
|----------|------|
| Full 60-min / 20-speaker production STT pack executed | Spec/tooling only unless field evidence is added |
| Three-listener blinded TTS study completed | Protocol only |
| Three-platform named-hardware Tier A retained baselines | Harness/policy only |
| Remotes beyond experimental without protected evidence | Honesty demotion retained |
| Batch source identity under concurrent open-file mutation mid-decode | Re-hash after decode; not a single open-file snapshot through decode |
| Product-proof programmes (JOE-2216–2218 evidence execution) | Still residual from independent audit |

## Machine-checkable gates

```bash
./scripts/version_check.sh
./scripts/check_product_contracts.sh
./scripts/check_provider_evidence.sh
./scripts/rc_freeze_check.sh
# Prefer full CI green on the release/* PR before merge/tag.
```

## Disposition

- **Do not yank v0.0.22.**  
- **Publish v0.0.23** as the recommended tip for SRT/batch/SDK correctness.  
- Product-proof packs remain follow-on work, not tag blockers for this patch.

## Post-publication verification (2026-08-03)

| Item | Value |
|------|--------|
| Tag | `v0.0.23` |
| Tag commit | `59408acf46a3e5ce58f6f3f92b2f0f447a622809` |
| GitHub Release | https://github.com/joe-broadhead/aurum/releases/tag/v0.0.23 |
| Release workflow | success (run 30857495411) |
| crates.io | `aurum-core` / `aurum-stt` / `aurum-ffi` **0.0.23** published |

