# Product-proof residuals after v0.0.23

**Programme:** [JOE-2315](https://linear.app/joe-broadhead/issue/JOE-2315)  
**Milestone:** v0.0.24+ — Product-proof evidence residuals  
**Published tip:** v0.0.23  

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

## Children

| Issue | Outcome required |
|-------|------------------|
| [JOE-2318](https://linear.app/joe-broadhead/issue/JOE-2318) | Production STT pack evaluated + scorecard retained |
| [JOE-2319](https://linear.app/joe-broadhead/issue/JOE-2319) | Three-listener blinded TTS study retained |
| [JOE-2317](https://linear.app/joe-broadhead/issue/JOE-2317) | Named-hardware Tier A reports on macOS / Linux / Windows |
| [JOE-2316](https://linear.app/joe-broadhead/issue/JOE-2316) | Process-owned batch source snapshot (engineering) |

## Operator entry points

```bash
# STT production pack (operator machine; licensed open audio only)
./scripts/eval/prepare_stt_observatory_corpus.sh --recipe-check
./scripts/eval/prepare_stt_observatory_corpus.sh --production

# Named-hardware perf capture (model must be cached)
./scripts/run_perf_report.sh --profile apple_silicon_metal --model tiny-q5_1

# TTS listening protocol
# docs/operations/tts-listening-protocol.md
```

## Related

* `docs/operations/v0023-product-acceptance.md` — v0.0.23 GO + residuals table  
* `docs/operations/v0022-product-acceptance.md` — prior programme gate  
