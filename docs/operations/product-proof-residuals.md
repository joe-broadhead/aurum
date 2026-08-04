# Product-proof residuals after v0.0.23

**Programme:** [JOE-2315](https://linear.app/joe-broadhead/issue/JOE-2315)  
**Milestone:** v0.0.24+ — Product-proof evidence residuals  
**Published tip:** v0.0.23 (`59408acf…`)  

v0.0.22 shipped infrastructure for quality/performance evidence. An independent
audit found several outcomes were **specified** but not **executed**. v0.0.23
fixed urgent correctness. This document tracks **product-proof** field work.

## Honest rule

| Allowed | Forbidden |
|---------|-----------|
| Retain real reports under `evals/reports/` | Mark Done on synthetic dry-runs alone |
| Fail closed on budget regression | Rewrite baselines solely to pass a release |
| Keep remotes experimental without live evidence | Claim world-class quality without packs |

## Status snapshot

| Issue | Status | Honest claim |
|-------|--------|--------------|
| [JOE-2316](https://linear.app/joe-broadhead/issue/JOE-2316) | **Done** | Process-owned batch source snapshot |
| [JOE-2317](https://linear.app/joe-broadhead/issue/JOE-2317) | **Done** | macOS + Linux + Windows Tier A field reports |
| [JOE-2318](https://linear.app/joe-broadhead/issue/JOE-2318) | **Done** | Production pack + 5-model subset matrix + fail-closed budgets |
| [JOE-2319](https://linear.app/joe-broadhead/issue/JOE-2319) | In Progress | Objective matrix Done; **3-listener study not completed** |

## JOE-2318 — STT production pack — **Done**

* Pack assemble + `--production` coverage gate (real open-licensed speech)
* **5-model production-subset matrix** (32 fixtures, multi-accent):

  | Model | speech-only WER | silence FP |
  |-------|-----------------|------------|
  | `tiny-q5_1` | ~0.132 | 2 |
  | `base` | ~0.100 | 2 |
  | `base.en-q5_1` | ~0.099 | 1 |
  | `small-q5_1` | ~0.094 | 2 |
  | `large-v3-turbo-q5_0` | ~0.060 | 1 |

* Fail-closed budgets: `evals/observatory/budgets/stt-*.production-subset.json`
* Matrix: `evals/reports/stt/stt-production-subset-matrix-apple_silicon_metal.md`

**Explicit non-claims:** full-hour fixture sweep; TED-LIUM / multilingual slots;
full-precision `large-v3-turbo` (quantized turbo retained).

## JOE-2319 — TTS listening — **awaiting humans**

* Kitten objective matrix all_passed (Luna/Jasper/Bella)
* Blinded session **round 003** ready (24 fixtures × 2 systems = 48 items)
* Operator handoff: `evals/reports/listening/listening-round-003-operator-handoff.md`
* **Blocking Done:** 3 independent listeners fill ratings → aggregate → Kitten disposition

## JOE-2317 — Named-hardware Tier A perf — **Done**

See `evals/reports/perf/README-tier-a.md`.

## Operator entry

```bash
# STT production subset + budget
./scripts/eval/prepare_stt_observatory_corpus.sh --score-subset --model base --max-fixtures 32
python3 scripts/eval/compare_stt_budget.py \
  --report evals/reports/stt/stt-production-subset-apple_silicon_metal-base.json \
  --budget evals/observatory/budgets/stt-base.production-subset.json

# TTS listening (after humans fill worksheets)
python3 scripts/eval/prepare_tts_listening_session.py aggregate \
  --session-dir evals/reports/_local/tts_listening_sessions/listening-round-003-blinded \
  --ratings path/to/ratings_L1.csv path/to/ratings_L2.csv path/to/ratings_L3.csv
```
