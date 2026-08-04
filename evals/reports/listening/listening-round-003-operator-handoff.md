# TTS listening round 003 — operator handoff (JOE-2319)

Evidence version: `0.0.22-tts-listening-v1`  
**Status:** session pack ready — **awaiting 3 independent human listeners**

## Session location (gitignored audio)

```text
evals/reports/_local/tts_listening_sessions/listening-round-003-blinded/
  audio/                 # 48 WAVs (24 fixtures × 2 blinded systems)
  ratings_worksheet.csv  # fill intelligibility / naturalness / pronunciation / joins
  ratings.template.jsonl
  session.json
  reveal.json            # OPERATOR ONLY until ratings complete
  README.md
```

## Blinded systems

| Label | Revealed after scoring |
|-------|------------------------|
| A | (see local `reveal.json`) |
| B | (see local `reveal.json`) |

Do **not** publish `reveal.json` or share it with listeners before ratings are in.

## Listener protocol (minimum)

| Rule | Value |
|------|-------|
| Independent listeners | **3** (`L1`, `L2`, `L3` only — no names/emails) |
| Scales | 1–5 intelligibility, naturalness, pronunciation, join smoothness |
| Critical failure | yes/no (omission, severe mispronunciation, clipping, unusable) |
| Playback | headphones preferred; same volume guidance for all |

## After three worksheets are filled

```bash
# Example: convert three filled CSVs into one JSONL, then aggregate
python3 scripts/eval/prepare_tts_listening_session.py aggregate \
  --session-dir evals/reports/_local/tts_listening_sessions/listening-round-003-blinded \
  --ratings \
    path/to/ratings_L1.csv \
    path/to/ratings_L2.csv \
    path/to/ratings_L3.csv \
  --report-dir evals/reports/listening
```

Aggregate JSON (listener_count ≥ 3) is the public retained artifact.  
Then record Kitten-default disposition against medians in
`evals/observatory/tts-default-decision.md`.

## Prerequisite objective evidence (already retained)

Kitten Luna / Jasper / Bella full-pack objective matrix: **all_passed**  
(`tts-objective-matrix-apple_silicon_metal.json`).

## Honesty

* This handoff is **not** a completed three-listener study.
* Round 002 remains available; round 003 is the current recommended pack (24 fixtures).
