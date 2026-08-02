# Evidence pack — v0.0.22 STT observatory (Wave 1)

Dated: **2026-08-02**  
Evidence version: **`0.0.22-observatory-v1`**  
Linear: JOE-2216 (parent JOE-2215)

## What shipped in Wave 1

* Versioned observatory corpus + report schemas in `aurum-core`
* Fail-closed budget comparison (Rust + `scripts/eval/compare_stt_budget.py`)
* Redistributable core corpus and production pack **recipe** (external open data)
* Profile evidence version pin to this pack
* Model/profile matrix under `evals/observatory/budgets/matrix.json`

## What is not claimed yet

* A full 60-minute multi-speaker human speech baseline **checked into CI** —
  the production pack is operator-prepared from licensed open sources.
* Automatic change of the global `base` default.
* Universal field WER from the synthetic core alone.

## How to extend to the full production baseline

1. `./scripts/eval/prepare_stt_observatory_corpus.sh --slot <name>` for each slot
2. Generate `evals/observatory/cache/corpus.production.json`
3. Run the local model matrix; retain reports under `evals/reports/stt/`
4. Update committed budgets with report diffs + changelog
5. Link the reviewed report on JOE-2215 / JOE-2226

## Privacy

No private user audio, Plaud exports, API keys, or absolute private paths in
retained public reports.
