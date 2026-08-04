# Product-proof residuals after v0.0.23

**Programme:** [JOE-2315](https://linear.app/joe-broadhead/issue/JOE-2315)  
**Milestone:** v0.0.24+ — Product-proof evidence residuals  
**Published tip:** v0.0.23 (`59408acf…`)  

v0.0.22 shipped infrastructure for quality/performance evidence. An independent
audit found several outcomes were **specified** but not **executed**. v0.0.23
fixed urgent correctness (SRT, long-form, batch identity, SDK/OpContext, native
SDK consumers). This document tracks the remaining **product-proof** work.

## Honest rule

| Allowed | Forbidden |
|---------|-----------|
| Retain real reports under `evals/reports/` | Mark Done on synthetic dry-runs alone |
| Fail closed on budget regression | Rewrite baselines to make a release pass |
| Keep remotes experimental without live evidence | Claim world-class quality without packs |

## Status snapshot

| Issue | Status | Honest claim |
|-------|--------|--------------|
| [JOE-2316](https://linear.app/joe-broadhead/issue/JOE-2316) | **Done** | Process-owned batch source snapshot (`36cbf54`, PR #99) |
| [JOE-2318](https://linear.app/joe-broadhead/issue/JOE-2318) | In Progress | Pack + 4-model subset matrix + budgets; TED/multi / full-hour still open |
| [JOE-2319](https://linear.app/joe-broadhead/issue/JOE-2319) | In Progress | Kitten objective matrix (PR #101); **3 listeners still required** |
| [JOE-2317](https://linear.app/joe-broadhead/issue/JOE-2317) | **Done** | macOS + Linux + Windows field reports retained; GHA limits documented |

## JOE-2318 — STT production pack

* Automated fetch/assemble for LibriSpeech, Common Voice accents, noise, long-form, controls
* `--production` coverage gate passed (~369 min, 66 speakers, 5 accents, 3 long-form)
* **Multi-model production-subset matrix** (32 fixtures, multi-accent):

  | Model | speech-only WER | silence FP | budget |
  |-------|-----------------|------------|--------|
  | `tiny-q5_1` | ~0.132 | 2 | `stt-tiny-q5_1.production-subset.json` |
  | `base` | ~0.100 | 2 | `stt-base.production-subset.json` |
  | `base.en-q5_1` | ~0.099 | 1 | `stt-base.en-q5_1.production-subset.json` |
  | `small-q5_1` | ~0.094 | 2 | `stt-small-q5_1.production-subset.json` |

* Reports: `evals/reports/stt/stt-production-subset-*.json` + matrix summary
* **Still open for Done:** `large-v3-turbo` (optional), TED-LIUM/multilingual slots, full-hour fixture sweep

## JOE-2319 — TTS listening

* Kitten Luna / Jasper / Bella objective matrix: **all_passed** (65 fixtures)
* Blinded session pack tooling ready; **3 listeners still required**
* See `docs/operations/tts-listening-protocol.md`

## JOE-2317 — Named-hardware Tier A perf — **Done**

* Capture tooling: `scripts/eval/run_tier_a_perf_capture.py` (schema 2)
* **Three-platform field reports retained** (see `evals/reports/perf/README-tier-a.md`):

  | Platform | Report | Notes |
  |----------|--------|-------|
  | macOS arm64 (maintainer) | `perf-tier_a_macos_arm64-field.json` | M4 local; doctor/STT/30s/TTS |
  | Linux x86_64 (GHA) | `perf-tier_a_linux_x86_64_gnu-gha.json` | EPYC; doctor/STT/30s/TTS |
  | macOS arm64 (GHA) | `perf-tier_a_macos_arm64_gha-gha.json` | M1 VM; doctor/STT/30s/TTS |
  | Windows x86_64 (GHA) | `perf-tier_a_windows_x86_64_msvc-gha.json` | doctor/CLI STT (prior GHA) |

* Budget seeds + compare PASS for each retained report
* GHA full re-run: https://github.com/joe-broadhead/aurum/actions/runs/30883816912
* **Out of scope for this Done:** full catalogue (concurrency/batch/large models);
  Windows illegal-instruction flake on later rebuild documented, not blocking family evidence

## Operator entry points

```bash
# STT production pack (operator machine; licensed open audio only)
./scripts/eval/prepare_stt_observatory_corpus.sh --recipe-check
./scripts/eval/prepare_stt_observatory_corpus.sh --fetch-slot all-auto
./scripts/eval/prepare_stt_observatory_corpus.sh --assemble-production
./scripts/eval/prepare_stt_observatory_corpus.sh --production
./scripts/eval/prepare_stt_observatory_corpus.sh --score-subset --model tiny-q5_1

# TTS objective + listening session
python3 scripts/eval/run_tts_objective_matrix.py --local-only
python3 scripts/eval/prepare_tts_listening_session.py prepare \
  --pairs kitten-nano-int8:Luna,kitten-nano-int8:Jasper --min-fixtures 20

# Named-hardware Tier A perf (model must be cached)
python3 scripts/eval/run_tier_a_perf_capture.py --profile-id tier_a_macos_arm64
python3 scripts/eval/compare_perf_budget.py \
  --report evals/reports/perf/perf-tier_a_macos_arm64-field.json \
  --budget evals/observatory/budgets/perf-tier_a_macos_arm64.field.json
```

## Related

* `docs/operations/v0023-product-acceptance.md` — v0.0.23 GO + residuals table  
* `docs/operations/v0022-product-acceptance.md` — prior programme gate  
* `docs/operations/stt-production-pack-operator.md`  
* `docs/operations/tts-listening-protocol.md`  
* `docs/operations/performance-reports.md`  
