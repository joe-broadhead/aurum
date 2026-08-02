# Aurum evaluation corpus (JOE-1607 / JOE-1731 / JOE-2216)

Offline, versioned fixtures for **quality** gates (WER/CER, silence FP, repetition,
TTS objective checks). Performance budgets: `docs/operations/benchmarks.md` and
`docs/operations/performance-reports.md` (JOE-1739).

**Production STT quality observatory (JOE-2216):** see `evals/observatory/` and
`docs/operations/stt-observatory.md`. Evidence version: `0.0.22-observatory-v1`.

## Corpora

| File | Role |
|------|------|
| `observatory/corpus.core.json` | Redistributable observatory core (schema + CI budgets) |
| `observatory/corpus.production.manifest.json` | Production pack recipe (external licensed speech) |
| `corpus.smoke.json` | Text-only smoke (PR unit tests) |
| `corpus.public-v1.json` | Offline public matrix with synthetic multi-accent speech + silence/noise |

```bash
python3 scripts/generate_eval_audio.py
./scripts/generate_eval_speech.sh   # macOS say + ffmpeg
AURUM_BIN=target/release/aurum python3 scripts/run_stt_eval_matrix.py
```

```rust
use aurum_core::eval::{score_stt, smoke_corpus, build_report};

let corpus = smoke_corpus();
let scores = corpus.stt.iter().map(|f| score_stt(f, &f.reference, true)).collect();
let report = build_report(&corpus, "tiny-q5_1", "asr", scores);
```

### Tags

`clean`, `short`, `long`, `numbers`, `silence`, `noise`, `non_speech`,
`accent_us` / `accent_gb` / `accent_au`, `punctuation`, `join`, `synthetic`.

### What this is not

- Licensed **human** multi-accent speech for field WER claims (extend the same schema).
- A MOS score — see `docs/operations/listening-scorecard.md` and
  `evals/reports/listening/listening-round-001.json` (pilot only).

## Reports

Retained baselines live under `evals/reports/` (see that directory’s README).
Interpretation: `docs/operations/evidence-v004.md`.

## Timestamp policy

- **ASR** (`backend_kind = asr`): timestamps may be scored for boundary error.
- **LLM-assisted**: mark `timestamps_reliable = false`; never fail a model solely
  on timestamp error for that path.
