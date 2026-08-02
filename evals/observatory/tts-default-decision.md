# TTS default model decision (JOE-2217)

**Date:** 2026-08-02  
**Evidence version:** `0.0.22-tts-listening-v1`  
**Decision:** **Retain Kitten (`kitten-nano-int8`) as the default local TTS model.**

## Rationale

* Kitten remains the smallest supported local pack with honest capability claims.
* Wave 1 ships the evaluation pack, objective evaluator, support-tier policy, and
  listening protocol; a full ≥3-listener blinded promotion study for a default
  change is a product follow-up, not an automatic switch.
* Kokoro remains available as an opt-in higher-quality local model when the pack
  is installed; it is not silently selected.
* Download size, cold-start cost, and named-hardware RTF (JOE-2218) must be
  reviewed before any default change.

## Conditions to revisit

1. Objective matrix green for both Kitten and Kokoro on the production pack.
2. Listening report with ≥3 independent blinded listeners; Kitten vs Kokoro
   pairwise preference recorded.
3. Performance report on Tier A hardware shows acceptable RTF/RSS for the
   candidate default.
4. Explicit changelog + docs update; no silent default rewrite.

## Related

* `docs/operations/tts-listening-protocol.md`
* `evals/observatory/tts_eval_pack.v1.json`
* Historical pilot: `evals/reports/listening/listening-round-001.json`
