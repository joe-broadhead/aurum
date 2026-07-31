# Aurum evaluation corpus (JOE-1607)

Offline, versioned fixtures for **quality** gates (WER/CER, TTS objective checks).
Performance budgets live under `docs/operations/benchmarks.md` (JOE-1606).

## Smoke corpus

`corpus.smoke.json` is redistributable synthetic text (CC0). It does **not** ship
copyrighted audio. Score text hypotheses with:

```rust
use aurum_core::eval::{score_stt, smoke_corpus, build_report};

let corpus = smoke_corpus();
// or: serde_json::from_str(include_str!("../../../evals/corpus.smoke.json"))
let scores = corpus.stt.iter().map(|f| score_stt(f, &f.reference, true)).collect();
let report = build_report(&corpus, "tiny-q5_1", "asr", scores);
```

## Adding a model

1. Run STT (or TTS) against the smoke corpus and any larger private corpora.
2. Record mean WER/CER (and tier-specific budgets) in the release notes.
3. Do **not** treat proxy metrics as MOS; use the listening scorecard in
   `docs/operations/evals.md` for subjective quality.

## Timestamp policy

- **ASR** (`backend_kind = asr`): timestamps may be scored for boundary error.
- **LLM-assisted**: mark `timestamps_reliable = false`; never fail a model solely
  on timestamp error for that path.
