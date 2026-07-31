# Aurum evaluation corpus (JOE-1607 / JOE-1731)

Offline, versioned fixtures for **quality** gates (WER/CER, silence FP, repetition,
TTS objective checks). Performance budgets: `docs/operations/benchmarks.md` and
`docs/operations/performance-reports.md` (JOE-1739).

## Smoke corpus (v2)

`corpus.smoke.json` is redistributable synthetic text (CC0) plus optional
**synthetic audio** under `audio/` (silence + tone, not speech).

```bash
python3 scripts/generate_eval_audio.py
```

```rust
use aurum_core::eval::{score_stt, smoke_corpus, build_report};

let corpus = smoke_corpus();
let scores = corpus.stt.iter().map(|f| score_stt(f, &f.reference, true)).collect();
let report = build_report(&corpus, "tiny-q5_1", "asr", scores);
```

### Tags

`clean`, `short`, `long`, `numbers`, `silence`, `noise`, `non_speech`, `accent` (placeholder),
`punctuation`, `join`.

### What this is not

- Multi-accent licensed speech for production WER claims (bring your own under the same schema).
- A MOS score — see `docs/operations/listening-scorecard.md`.

## Reports

Operator reports may be written under `evals/reports/` (typically gitignored).
Templates/docs: listening scorecard + performance report methodology in
`docs/operations/`.

## Timestamp policy

- **ASR** (`backend_kind = asr`): timestamps may be scored for boundary error.
- **LLM-assisted**: mark `timestamps_reliable = false`; never fail a model solely
  on timestamp error for that path.
