# Aurum OpenRouter chunked re-bench

- **generated_at_utc:** 2026-08-01T19:03:33Z
- **aurum:** aurum 0.0.17
- **chunk_sec:** 210.0
- **chunks:** 4
- **audio:** 685.1s Matt Pocock lecture
- **path:** `aurum transcribe --provider openrouter --openrouter-stt-mode transcriptions` per chunk, stitch

This exercises the **real Aurum CLI remote path** (not direct OpenRouter API),
working around `max_segment_chars=8000` via client-side chunking.

## Leaderboard (mean WER vs Plaud + YouTube)

| Rank | Model | Status | Mean WER | vs Plaud | vs YouTube | Chunks | Wall s |
|-----:|-------|--------|---------:|---------:|-----------:|-------:|-------:|
| 1 | `fish-audio/transcribe-1` | pass | 1.85% | 1.18% | 2.52% | 4/4 | 33.55 |
| 2 | `mistralai/voxtral-mini-transcribe` | pass | 1.92% | 1.36% | 2.47% | 4/4 | 22.98 |
| 3 | `microsoft/mai-transcribe-1.5` | pass | 1.98% | 1.71% | 2.25% | 4/4 | 21.25 |
| 4 | `openai/gpt-4o-transcribe` | pass | 2.05% | 1.58% | 2.52% | 4/4 | 48.9 |
| 5 | `qwen/qwen3-asr-flash-2026-02-10` | pass | 2.09% | 1.62% | 2.56% | 4/4 | 118.8 |
| 6 | `openai/whisper-1` | pass | 2.14% | 1.75% | 2.52% | 4/4 | 52.36 |
| 7 | `openai/gpt-4o-mini-transcribe` | pass | 2.44% | 1.93% | 2.96% | 4/4 | 34.35 |
| 8 | `openai/whisper-large-v3-turbo` | pass | 2.73% | 2.37% | 3.09% | 4/4 | 18.13 |
| 9 | `nvidia/parakeet-tdt-0.6b-v3` | pass | 3.37% | 2.98% | 3.75% | 4/4 | 18.27 |
| 10 | `deepgram/nova-3` | pass | 4.18% | 3.81% | 4.55% | 4/4 | 18.37 |
| 11 | `x-ai/grok-stt-1.0` | pass | 4.51% | 4.03% | 4.99% | 4/4 | 18.77 |
| 12 | `openai/whisper-large-v3` | pass | 14.61% | 14.20% | 15.01% | 4/4 | 24.3 |
| 13 | `google/chirp-3` | partial | 94.72% | 94.04% | 95.41% | 1/4 | 41.18 |

## Failures / partials

- `google/chirp-3` (partial): chunk0:rc=4:error: openrouter returned an error: HTTP 400 Bad Request: {"error":{"message":"Provider returned 400","code":400}}; chunk1:rc=4:error: openrouter returned an error: HTTP 400 Bad Request: 

## Notes

- Dual-ref mean WER matches the better-eval methodology.
- Compare to `EVAL_REPORT.md` (direct OpenRouter API hyps) to see CLI path parity.
- Product fix options: raise `max_segment_chars`, or document chunking for long audio.

Rows: `results_openrouter_chunked.jsonl`. Hyps: `hypotheses/aurum_chunked_*.txt`.

