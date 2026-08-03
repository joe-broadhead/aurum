# STT quality observatory (JOE-2216)

Authoritative, versioned evidence for local STT quality: corpus schema, validators,
scorecards, and fail-closed budget comparison.

## Layout

| Path | Role |
|------|------|
| `corpus.core.json` | Redistributable synthetic/control core (CI-safe) |
| `corpus.production.manifest.json` | Production pack metadata + external asset recipe |
| `budgets/` | Committed baselines (not generated reports) |
| `budgets/matrix.json` | Local model/profile matrix |
| `../reports/stt/` | Retained real-model reports (machine-readable) |
| `../../docs/operations/stt-observatory.md` | Methodology and release-gate usage |

## Contracts (summary)

* **Schema** — `ObservatoryCorpus` / `ObservatoryReport` in `aurum_core::eval::observatory`
  (`schema_version = 1`, evidence `0.0.22-observatory-v1`).
* **Coverage (production pack)** — ≥60 minutes licensed real speech, ≥20 speakers,
  clean + lecture + noisy, ≥4 English accents, numbers/dates/acronyms, silence and
  non-speech controls, multilingual/code-switch subset, ≥3 long-form (>10 min).
* **Budget** — aggregate WER may not regress by more than **10% relative** or
  **1.0 absolute WER point** (whichever is larger); scenario groups **15% relative**;
  no new silence false positives; repetition above threshold fails; timestamp MAE
  budget when the backend claims reliability.
* **Privacy** — reports retain fixture IDs and metrics only — never raw audio,
  private transcripts, keys, or absolute private paths. CI never depends on Plaud
  or other private material.

## Commands

Validate the redistributable core (no external assets):

```bash
cargo test -p aurum-core observatory -- --nocapture
python3 scripts/eval/compare_stt_budget.py --help
```

Compare a candidate report to a committed budget (non-zero on failure):

```bash
python3 scripts/eval/compare_stt_budget.py \
  --report evals/reports/stt/stt-apple_silicon_metal-tiny-q5_1.json \
  --budget evals/observatory/budgets/stt-tiny-q5_1.core.json
```

Prepare the full production pack (operator machine; verifies digests):

```bash
./scripts/eval/prepare_stt_observatory_corpus.sh --help
# Real licensed fetch (JOE-2318) — audio stays under cache/ (gitignored):
./scripts/eval/prepare_stt_observatory_corpus.sh --fetch-slot all-auto
./scripts/eval/prepare_stt_observatory_corpus.sh --assemble-production
./scripts/eval/prepare_stt_observatory_corpus.sh --production
./scripts/eval/prepare_stt_observatory_corpus.sh --score-subset --model tiny-q5_1
```

Helper: `scripts/eval/fetch_production_slots.py`. Operator notes:
`docs/operations/stt-production-pack-operator.md` and
`docs/operations/product-proof-residuals.md`.

## Profile evidence

`PROFILE_EVIDENCE_VERSION` in `aurum-core` is pinned to
`0.0.22-observatory-v1` and must cite a reviewed observatory report when mappings
change. Explicit `--model` remains authoritative over profiles.

## What this is not

* A claim of universal field quality from the synthetic core alone.
* Permission to check private user recordings into the repository.
* Automatic changes to the global `base` default.
