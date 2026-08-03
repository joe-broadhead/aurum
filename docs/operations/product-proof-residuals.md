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
| [JOE-2316](https://linear.app/joe-broadhead/issue/JOE-2316) | **Done** | Process-owned batch source snapshot merged (`36cbf54`, PR #99) |
| [JOE-2318](https://linear.app/joe-broadhead/issue/JOE-2318) | In Progress | Real STT pack assembled + subset scored; full matrix open |
| [JOE-2319](https://linear.app/joe-broadhead/issue/JOE-2319) | In Progress | Kitten objective matrix retained; **3-listener study not completed** |
| [JOE-2317](https://linear.app/joe-broadhead/issue/JOE-2317) | Partial | macOS arm64 field capture expanded; Linux/Windows still open |

## JOE-2318 — STT production pack

### Field progress

1. Automated fetch of open-licensed slots via
   `scripts/eval/fetch_production_slots.py` and
   `prepare_stt_observatory_corpus.sh --fetch-slot …`
2. Assembled `evals/observatory/cache/corpus.production.json` (gitignored audio)
3. `--production` coverage gate **passed** (~369 min, 66 speakers, 5 accents, 3 long-form)
4. Scored a 24-fixture production subset on local `tiny-q5_1`

### Retained report

* `evals/reports/stt/stt-production-subset-apple_silicon_metal-tiny-q5_1.json`
* Speech-only mean WER ≈ **0.105**; silence FP = **2** (retained honestly)

### Slot automation

| Slot | Automated? | Source |
|------|------------|--------|
| `librispeech_clean_subset` | Yes | OpenSLR-12 test-clean (CC BY 4.0) |
| `common_voice_accents` | Yes | `fsicoli/common_voice_17_0` en/dev (CC0) |
| `musan_noise_mix` | Yes (disk-friendly) | LS + ffmpeg white-noise overlay |
| `long_form_assemblies` | Yes | LS multi-utt concat |
| `controls_silence_nonspeech` | Yes | In-repo synthetic CC0 |
| `tedlium_lecture` | Recipe only | NC — do not re-upload as public CI |
| `multilingual_codeswitch` | Recipe only | Pending |

### Still open for Done

* Full model matrix on production pack
* TED-LIUM / multilingual slots (or coverage re-plan)
* Production budget baseline + full fixture sweep

## JOE-2319 — TTS listening

* Kitten Luna / Jasper / Bella objective matrix: **all_passed** (65 fixtures)
* Blinded session pack tooling ready; **3 listeners still required**
* See `docs/operations/tts-listening-protocol.md`

## JOE-2317 — Named-hardware Tier A perf

* macOS arm64 operator field reports retained under `evals/reports/perf/`
* Linux x86_64 and Windows MSVC Tier A baselines still open
* See `docs/operations/performance-reports.md`

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

# Named-hardware perf capture (model must be cached)
./scripts/eval/run_tier_a_perf_capture.py --profile apple_silicon_metal
```

## Related

* `docs/operations/v0023-product-acceptance.md` — v0.0.23 GO + residuals table  
* `docs/operations/v0022-product-acceptance.md` — prior programme gate  
* `docs/operations/stt-production-pack-operator.md`  
* `docs/operations/tts-listening-protocol.md`  
* `docs/operations/performance-reports.md`  
