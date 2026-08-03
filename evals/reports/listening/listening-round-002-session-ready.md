# Listening round 002 — session ready (awaiting listeners)

| Field | Value |
|-------|-------|
| Evidence version | `0.0.22-tts-listening-v1` |
| Session id | `listening-round-002-blinded` |
| Blinding | Yes (labels A/B) |
| Fixtures | 20 (category-diverse from `tts_eval_pack.v1`) |
| Systems | 2 Kitten voices (default + male) |
| Presentation items | 40 (shuffled) |
| Listeners required | **3** |
| Status | **Pack prepared — ratings not yet collected** |

## Operator location (not committed)

```text
evals/reports/_local/tts_listening_sessions/listening-round-002-blinded/
  session.json
  reveal.json          # operator only until ratings complete
  ratings_worksheet.csv
  ratings.template.jsonl
  audio/*.wav
  README.md
```

## After three listeners

```bash
python3 scripts/eval/prepare_tts_listening_session.py aggregate \
  --session-dir evals/reports/_local/tts_listening_sessions/listening-round-002-blinded \
  --ratings path/to/ratings.jsonl
```

Aggregate JSON is the public retained artifact. This markdown is **not** a
completed three-listener study.

## Objective prerequisite

Kitten Luna / Jasper / Bella full-pack objective matrix: **all_passed**
(`tts-objective-matrix-apple_silicon_metal.json`).
