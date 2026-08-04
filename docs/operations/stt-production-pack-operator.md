# STT production pack — operator path (JOE-2231 / JOE-2318)

The redistributable **core** corpus runs in CI. The **production** pack
(≥60 minutes, multi-speaker, multi-accent, long-form) is assembled on an
operator machine from **licensed open sources** documented in
`evals/observatory/corpus.production.manifest.json`.

Audio under `evals/observatory/cache/` is **gitignored** and not redistributed.

## Quick commands

```bash
# CI-safe
./scripts/eval/prepare_stt_observatory_corpus.sh --core
./scripts/eval/prepare_stt_observatory_corpus.sh --recipe-check

# Prove coverage checker with synthetic fixtures (NOT real speech evidence)
./scripts/eval/prepare_stt_observatory_corpus.sh \
  --dry-run-production --cache-dir /tmp/aurum-obs-cache

# Operator machine — automated real fetch (JOE-2318)
./scripts/eval/prepare_stt_observatory_corpus.sh --fetch-slot all-auto
./scripts/eval/prepare_stt_observatory_corpus.sh --assemble-production
./scripts/eval/prepare_stt_observatory_corpus.sh --production

# Score a capped real-speech subset (local model must be cached)
./scripts/eval/prepare_stt_observatory_corpus.sh --score-subset \
  --model tiny-q5_1 --profile apple_silicon_metal --max-fixtures 32
./scripts/eval/prepare_stt_observatory_corpus.sh --score-subset \
  --model base --profile apple_silicon_metal --max-fixtures 32

# Fail-closed budget compare against production-subset baselines
python3 scripts/eval/compare_stt_budget.py \
  --report evals/reports/stt/stt-production-subset-apple_silicon_metal-base.json \
  --budget evals/observatory/budgets/stt-base.production-subset.json

# Per-slot recipe only (no download)
./scripts/eval/prepare_stt_observatory_corpus.sh --slot librispeech_clean_subset
```

Automated slots today: `librispeech_clean_subset` (OpenSLR-12 test-clean),
`controls_silence_nonspeech`, `musan_noise_mix` (ffmpeg overlay; disk-friendly),
`long_form_assemblies`, and best-effort `common_voice_accents` (requires
`pip install datasets` + HF network). TED-LIUM and multilingual remain
recipe-documented until automated fetch is added.

## Operator sequence

1. `./scripts/eval/prepare_stt_observatory_corpus.sh --recipe-check`
2. `--fetch-slot all-auto` (or per-slot) → writes `cache/<slot>/` + digests
3. `--assemble-production` → `cache/corpus.production.json`
4. `--production` → enforce coverage minima (fail closed if incomplete)
5. `--score-subset` and/or full local matrix; retain redacted reports under
   `evals/reports/stt/`
6. Link reviewed reports on the release evidence index (no private paths/payloads)

## Honesty rules

| Claim | Allowed? |
|-------|----------|
| Core corpus schema/budget CI | Yes |
| Dry-run production pack meets coverage minima | Yes (coverage gate only) |
| Dry-run as field WER evidence | **No** |
| Private Plaud/user audio in git | **No** |
| Non-commercial TED-LIUM in public CI artifacts | **No** |

## Related

* [stt-observatory.md](stt-observatory.md)
* [evidence-v0022-observatory.md](evidence-v0022-observatory.md)
* [v0022-product-acceptance.md](v0022-product-acceptance.md)
