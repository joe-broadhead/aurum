# Quality evals (JOE-1607)

## STT metrics

| Metric | Definition |
|--------|------------|
| WER | Word error rate after normalization (case-fold, strip punctuation) |
| CER | Character error rate on the same normalized text |
| Empty/silence FP | Non-empty hypothesis when reference is empty |

LLM-assisted paths: measure verbatim divergence separately; **do not** use
timestamp error as a hard fail when `timestamps_reliable` is false.

## TTS objective checks

- Duration within fixture min/max
- Non-empty PCM, peak within guard
- Chunk-join continuity (no empty mid-utterance)

**Listening scorecard (human, not MOS):** naturalness, intelligibility,
pronunciation of numbers/abbreviations, join artifacts. Score 1–5 with notes;
never publish proxy metrics as MOS.

## Release gates

| Tier | Example models | Soft WER budget (smoke) |
|------|----------------|-------------------------|
| Tiny / trial | `tiny-q5_1` | informational only |
| Default | product default | track delta vs previous release |
| Large | optional large ggml | track delta vs previous release |

Adding or updating a model requires an eval report JSON (`EvalReport`) and an
explicit quality delta in the changelog.
