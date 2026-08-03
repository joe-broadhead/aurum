# STT production pack — operator path (JOE-2231)

The redistributable **core** corpus runs in CI. The **production** pack
(≥60 minutes, multi-speaker, multi-accent, long-form) is assembled on an
operator machine from **licensed open sources** documented in
`evals/observatory/corpus.production.manifest.json`.

## Quick commands

```bash
# CI-safe
./scripts/eval/prepare_stt_observatory_corpus.sh --core
./scripts/eval/prepare_stt_observatory_corpus.sh --recipe-check

# Prove coverage checker with synthetic fixtures (NOT real speech evidence)
./scripts/eval/prepare_stt_observatory_corpus.sh \
  --dry-run-production --cache-dir /tmp/aurum-obs-cache

# Operator machine — per slot
./scripts/eval/prepare_stt_observatory_corpus.sh --slot librispeech_clean_subset
# 1) download licensed sources for that slot only
# 2) write cache/<slot>/SHA256SUMS
# 3) assemble fixture rows into evals/observatory/cache/corpus.production.json
./scripts/eval/prepare_stt_observatory_corpus.sh --production
```

## Operator sequence

1. `./scripts/eval/prepare_stt_observatory_corpus.sh --recipe-check`
2. For each `asset_slot`: `--slot <id>` → fetch licensed audio → lockfile digests
3. Assemble `evals/observatory/cache/corpus.production.json` (not committed if license forbids)
4. `./scripts/eval/prepare_stt_observatory_corpus.sh --production`
5. Run local model matrix; retain redacted reports under `evals/reports/stt/`
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
