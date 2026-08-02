# STT quality observatory

**Issue:** JOE-2216  
**Evidence version:** `0.0.22-observatory-v1`  
**Code:** `aurum_core::eval::observatory`

## Purpose

Answer—with retained, reproducible evidence—how supported local models behave
across real speakers, noise, accents, multilingual speech, long-form audio and
timestamp conditions. Profile recommendations and the v0.0.22 release gate
consume this observatory.

## What is in-repo

| Artifact | Path |
|----------|------|
| Core corpus (CI) | `evals/observatory/corpus.core.json` |
| Production pack recipe | `evals/observatory/corpus.production.manifest.json` |
| Committed budgets | `evals/observatory/budgets/` |
| Model/profile matrix | `evals/observatory/budgets/matrix.json` |
| Retained real-model reports | `evals/reports/stt/` |
| Compare tool | `scripts/eval/compare_stt_budget.py` |
| Prepare / validate | `scripts/eval/prepare_stt_observatory_corpus.sh` |

The redistributable **core** is synthetic/control text and in-repo silence/tone
audio. It is sufficient for schema, scorecard, and budget **negative tests**.

The **production pack** (≥60 minutes, ≥20 speakers, accent/noise/long-form
coverage) is fetched on operator machines via documented open-data slots. CI
never depends on private Plaud material or restrictive-license uploads.

## Metrics

Per fixture and aggregate:

* WER / CER (normalization policy `normalize_v1_lower_alnum_ws`)
* empty hypothesis, silence false positive, repetition ratio
* transcript length ratio
* optional timestamp MAE and long-form boundary error
* processing duration and RTF (cross-link to the performance programme)

Reports are deterministic (sorted fixture IDs, stable JSON key order via
`BTreeMap` scenario groups) and **must not** embed raw hypotheses in the
public budget path (legacy retained reports may still include them for
maintainer diagnosis; new observatory reports omit payload text).

## Budget policy

For the same model on a comparable corpus version:

| Check | Threshold |
|-------|-----------|
| Aggregate mean WER | max(**10% relative**, **+1.0 absolute WER points**) |
| Scenario group mean WER | **15% relative** |
| Silence false positives | no new FP without product review |
| Mean repetition ratio | budget field (default 0.35) |
| Timestamp MAE | when backend claims reliability and budget sets a max |

Baseline updates require a before/after report, rationale, and changelog entry.
Thresholds may tighten after the first complete production baseline; they may
not be loosened solely to make a candidate pass.

## Local matrix

Required retained local models: `tiny-q5_1`, `base`, `base.en` (English subset),
`small`, `small.en` (English subset), `large-v3-turbo`, and every model selected
by `speed|balance|quality` profiles.

Remote providers run in protected/scheduled jobs only; their scorecards are
informational and do not block deterministic local release readiness.

## Profile integration

`PROFILE_EVIDENCE_VERSION` equals `0.0.22-observatory-v1`. Explicit `--model`
always wins. Experimental models stay excluded from profiles. The global
product default remains `base` until a separate reviewed decision changes it.

## Release gate

```bash
./scripts/eval/prepare_stt_observatory_corpus.sh --core
cargo test -p aurum-core observatory
python3 scripts/eval/compare_stt_budget.py \
  --report path/to/candidate.json \
  --budget evals/observatory/budgets/stt-tiny-q5_1.core.json
```

A non-zero compare exit fails the quality gate.

## Related

* `docs/guide/models.md` — scenario guidance
* `docs/operations/provider-qualification.md` — remote lanes
* `docs/operations/evidence-v004.md` — historical synthetic pack
* JOE-2218 — named-hardware performance (RTF fields)
* JOE-2219 — long-form boundary-aware STT (consumes boundary metrics)
