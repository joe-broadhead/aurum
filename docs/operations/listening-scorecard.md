# TTS listening scorecard (JOE-1735 / JOE-2217)

**Not MOS.** Use the production protocol in
[`tts-listening-protocol.md`](tts-listening-protocol.md) for promotion decisions.

This form remains useful for ad-hoc sessions. Record method, listener count, and
date. Prefer blinded labels and ≥3 listeners for any support-tier claim.

## Session metadata

| Field | Value |
|-------|-------|
| Date | |
| Aurum version / commit | |
| Adapters / models / voices | |
| Listener count (≥3 for promotion) | |
| Blinding (yes/no + method) | |
| Playback normalization | |
| Hardware (headphones guidance) | |
| Evidence version | `0.0.22-tts-listening-v1` |

## Items (score 1–5)

Select ≥20 fixtures from `evals/observatory/tts_eval_pack.v1.json` per promoted
model. Score intelligibility, naturalness, pronunciation, join smoothness; flag
critical failures.

| Fixture id | Blinded label | Intelligibility | Naturalness | Pronunciation | Join | Critical? | Notes |
|------------|---------------|-----------------|-------------|---------------|------|-----------|-------|
| | | | | | | | |

## Disposition

- Production-quality / support-tier candidate? Y/N
- Blocking issues:
- Default-model change requested? Y/N (requires separate decision record)
- Follow-ups:

Objective PCM checks: `aurum_core::eval::score_tts_pcm` / `TtsObjectiveReport`.
Support-tier gate: `evaluate_support_tier`.

## Round 001 (historical pilot)

See `evals/reports/listening/listening-round-001.json` and
[evidence-v004.md](evidence-v004.md). Round 001 is a **single-listener,
non-blinded pilot** — not sufficient for support-tier promotion under JOE-2217.
