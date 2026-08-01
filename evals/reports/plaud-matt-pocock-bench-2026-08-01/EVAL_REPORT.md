# Dual-reference STT eval — Matt Pocock lecture

- **generated_at_utc:** 2026-08-01T19:03:56Z
- **video:** https://www.youtube.com/watch?v=n0VhIVtviC0
- **plaud file:** `42e86284875bf900d98cb7e83ce3b7cb` (685s)
- **ref A (Plaud raw):** 2281 words
- **ref B (YouTube auto captions):** 2265 words
- **ref cross-WER (YT vs Plaud):** 2.41% (refs disagree by ~this much — floor for single-ref claims)

## Why dual-reference

The first bench used **only Plaud** as reference. That favors systems that
share Plaud’s ASR/cleanup DNA and treats Plaud errors as model errors.
YouTube auto-captions are an **independent** pipeline on the same speech.
Ranking by **mean WER** across both refs is a better quality proxy without a
human gold transcript.

## Leaderboard (by mean WER ↑ better)

| Rank | System | Model | Mean WER | vs Plaud | vs YouTube | Δ(Plaud−YT) | Words | Wall s |
|-----:|--------|-------|---------:|---------:|-----------:|------------:|------:|-------:|
| 1 | openrouter | `microsoft/mai-transcribe-1.5` | 1.63% | 1.36% | 1.90% | -0.54% | 2289 | 4.9 |
| 2 | other | `aurum_chunked_fish-audio_transcribe-1` | 1.85% | 1.18% | 2.52% | -1.33% | 2279 | — |
| 3 | aurum-local | `medium.en` | 1.87% | 1.45% | 2.30% | -0.85% | 2282 | 82.7 |
| 4 | other | `aurum_chunked_mistralai_voxtral-mini-transcribe` | 1.92% | 1.36% | 2.47% | -1.11% | 2279 | — |
| 5 | openrouter | `mistralai/voxtral-mini-transcribe` | 1.92% | 1.36% | 2.47% | -1.11% | 2279 | 7.5 |
| 6 | openrouter | `fish-audio/transcribe-1` | 1.92% | 1.23% | 2.60% | -1.38% | 2282 | 18.9 |
| 7 | aurum-local | `large-v3-turbo` | 1.94% | 1.53% | 2.34% | -0.81% | 2282 | 61.9 |
| 8 | other | `aurum_chunked_microsoft_mai-transcribe-1.5` | 1.98% | 1.71% | 2.25% | -0.54% | 2287 | — |
| 9 | other | `aurum_chunked_openai_gpt-4o-transcribe` | 2.05% | 1.58% | 2.52% | -0.94% | 2277 | — |
| 10 | other | `aurum_chunked_qwen_qwen3-asr-flash-2026-02-10` | 2.09% | 1.62% | 2.56% | -0.94% | 2277 | — |
| 11 | other | `aurum_chunked_openai_whisper-1` | 2.14% | 1.75% | 2.52% | -0.76% | 2279 | — |
| 12 | aurum-local | `small.en` | 2.31% | 1.71% | 2.91% | -1.20% | 2276 | 31.2 |
| 13 | openrouter | `openai/whisper-1` | 2.38% | 1.93% | 2.83% | -0.90% | 2279 | 40.3 |
| 14 | other | `aurum_chunked_openai_gpt-4o-mini-transcribe` | 2.44% | 1.93% | 2.96% | -1.03% | 2272 | — |
| 15 | other | `aurum_chunked_openai_whisper-large-v3-turbo` | 2.73% | 2.37% | 3.09% | -0.72% | 2279 | — |
| 16 | openrouter | `openai/whisper-large-v3-turbo` | 2.84% | 2.54% | 3.13% | -0.59% | 2285 | 2.2 |
| 17 | openrouter | `nvidia/parakeet-tdt-0.6b-v3` | 3.06% | 2.63% | 3.49% | -0.86% | 2277 | 2.8 |
| 18 | aurum-local | `small` | 3.19% | 2.67% | 3.71% | -1.03% | 2273 | 30.9 |
| 19 | aurum-local | `large-v3-turbo-q5_0` | 3.28% | 2.94% | 3.62% | -0.68% | 2283 | 56.1 |
| 20 | other | `aurum_chunked_nvidia_parakeet-tdt-0.6b-v3` | 3.37% | 2.98% | 3.75% | -0.77% | 2271 | — |
| 21 | openrouter | `deepgram/nova-3` | 3.37% | 2.89% | 3.84% | -0.95% | 2258 | 2.2 |
| 22 | other | `aurum_chunked_deepgram_nova-3` | 4.18% | 3.81% | 4.55% | -0.73% | 2261 | — |
| 23 | openrouter | `openai/whisper-large-v3` | 4.38% | 4.03% | 4.72% | -0.69% | 2223 | 3.4 |
| 24 | other | `aurum_chunked_x-ai_grok-stt-1.0` | 4.51% | 4.03% | 4.99% | -0.96% | 2265 | — |
| 25 | aurum-local | `base.en` | 4.86% | 4.34% | 5.39% | -1.05% | 2270 | 13.5 |
| 26 | aurum-local | `base` | 6.18% | 5.74% | 6.62% | -0.88% | 2275 | 12.2 |
| 27 | openrouter | `x-ai/grok-stt-1.0` | 6.62% | 6.31% | 6.93% | -0.62% | 2199 | 4.2 |
| 28 | aurum-local | `tiny-q5_1` | 10.74% | 10.13% | 11.35% | -1.22% | 2280 | 7.7 |
| 29 | other | `aurum_chunked_openai_whisper-large-v3` | 14.61% | 14.20% | 15.01% | -0.81% | 2013 | — |
| 30 | openrouter | `openai/gpt-4o-mini-transcribe` | 22.81% | 22.93% | 22.69% | 0.24% | 1793 | 16.0 |
| 31 | openrouter | `openai/gpt-4o-transcribe` | 22.85% | 23.19% | 22.52% | 0.68% | 1779 | 25.9 |
| 32 | other | `aurum_chunked_google_chirp-3` | 94.72% | 94.04% | 95.41% | -1.37% | 145 | — |

## Best by reference

- **Closest to Plaud:** `aurum_chunked_fish-audio_transcribe-1` (1.18%)
- **Closest to YouTube:** `microsoft/mai-transcribe-1.5` (1.90%)
- **Best mean:** `microsoft/mai-transcribe-1.5` (1.63%)

## Aurum local only

| Model | Mean WER | vs Plaud | vs YouTube | Wall s |
|-------|---------:|---------:|-----------:|-------:|
| `medium.en` | 1.87% | 1.45% | 2.30% | 82.7 |
| `large-v3-turbo` | 1.94% | 1.53% | 2.34% | 61.9 |
| `small.en` | 2.31% | 1.71% | 2.91% | 31.2 |
| `small` | 3.19% | 2.67% | 3.71% | 30.9 |
| `large-v3-turbo-q5_0` | 3.28% | 2.94% | 3.62% | 56.1 |
| `base.en` | 4.86% | 4.34% | 5.39% | 13.5 |
| `base` | 6.18% | 5.74% | 6.62% | 12.2 |
| `tiny-q5_1` | 10.74% | 10.13% | 11.35% | 7.7 |

## Error shape (vs YouTube — independent ref)

| Model | WER | Sub | Del | Ins | Hits |
|-------|----:|----:|----:|----:|-----:|
| `microsoft/mai-transcribe-1.5` | 1.90% | 13 | 3 | 27 | 2249 |
| `aurum_chunked_fish-audio_transcribe-1` | 2.52% | 15 | 14 | 28 | 2236 |
| `medium.en` | 2.30% | 15 | 10 | 27 | 2240 |
| `aurum_chunked_mistralai_voxtral-mini-transcribe` | 2.47% | 16 | 13 | 27 | 2236 |
| `mistralai/voxtral-mini-transcribe` | 2.47% | 16 | 13 | 27 | 2236 |
| `fish-audio/transcribe-1` | 2.60% | 20 | 11 | 28 | 2234 |
| `large-v3-turbo` | 2.34% | 14 | 11 | 28 | 2240 |
| `aurum_chunked_microsoft_mai-transcribe-1.5` | 2.25% | 17 | 6 | 28 | 2242 |
| `aurum_chunked_openai_gpt-4o-transcribe` | 2.52% | 15 | 15 | 27 | 2235 |
| `aurum_chunked_qwen_qwen3-asr-flash-2026-02-10` | 2.56% | 18 | 14 | 26 | 2233 |
| `aurum_chunked_openai_whisper-1` | 2.52% | 13 | 15 | 29 | 2237 |
| `small.en` | 2.91% | 21 | 17 | 28 | 2227 |
| `openai/whisper-1` | 2.83% | 20 | 15 | 29 | 2230 |
| `aurum_chunked_openai_gpt-4o-mini-transcribe` | 2.96% | 16 | 22 | 29 | 2227 |
| `aurum_chunked_openai_whisper-large-v3-turbo` | 3.09% | 28 | 14 | 28 | 2223 |
| `openai/whisper-large-v3-turbo` | 3.13% | 25 | 13 | 33 | 2227 |
| `nvidia/parakeet-tdt-0.6b-v3` | 3.49% | 27 | 20 | 32 | 2218 |
| `small` | 3.71% | 28 | 24 | 32 | 2213 |
| `large-v3-turbo-q5_0` | 3.62% | 24 | 20 | 38 | 2221 |
| `aurum_chunked_nvidia_parakeet-tdt-0.6b-v3` | 3.75% | 33 | 23 | 29 | 2209 |
| `deepgram/nova-3` | 3.84% | 26 | 34 | 27 | 2205 |
| `aurum_chunked_deepgram_nova-3` | 4.55% | 43 | 32 | 28 | 2190 |
| `openai/whisper-large-v3` | 4.72% | 15 | 67 | 25 | 2183 |
| `aurum_chunked_x-ai_grok-stt-1.0` | 4.99% | 39 | 37 | 37 | 2189 |
| `base.en` | 5.39% | 47 | 35 | 40 | 2183 |
| `base` | 6.62% | 70 | 35 | 45 | 2160 |
| `x-ai/grok-stt-1.0` | 6.93% | 39 | 92 | 26 | 2134 |
| `tiny-q5_1` | 11.35% | 132 | 55 | 70 | 2078 |
| `aurum_chunked_openai_whisper-large-v3` | 15.01% | 36 | 278 | 26 | 1951 |
| `openai/gpt-4o-mini-transcribe` | 22.69% | 24 | 481 | 9 | 1760 |
| `openai/gpt-4o-transcribe` | 22.52% | 12 | 492 | 6 | 1761 |
| `aurum_chunked_google_chirp-3` | 95.41% | 1 | 2140 | 20 | 124 |

## Pipeline affinity (bias)

`Δ = WER_plaud − WER_youtube`. Negative ⇒ more Plaud-like than YT-like.

- **Most Plaud-like:** `fish-audio/transcribe-1` Δ=-1.38% (P 1.23% / Y 2.60%)
  - `fish-audio/transcribe-1` Δ=-1.38%
  - `aurum_chunked_google_chirp-3` Δ=-1.37%
  - `aurum_chunked_fish-audio_transcribe-1` Δ=-1.33%

- **More YT-like / worse on Plaud:** `microsoft/mai-transcribe-1.5` Δ=-0.54%
- **More YT-like / worse on Plaud:** `openai/gpt-4o-mini-transcribe` Δ=0.24%
- **More YT-like / worse on Plaud:** `openai/gpt-4o-transcribe` Δ=0.68%

## Length warnings (possible dropouts / summarization)

- `aurum_chunked_openai_whisper-large-v3`: 2013 words (ratio vs Plaud 0.88)
- `openai/gpt-4o-mini-transcribe`: 1793 words (ratio vs Plaud 0.79)
- `openai/gpt-4o-transcribe`: 1779 words (ratio vs Plaud 0.78)
- `aurum_chunked_google_chirp-3`: 145 words (ratio vs Plaud 0.06)

## Method notes

1. Hypotheses from prior bench (Aurum local whisper.cpp + OpenRouter transcription models).
2. References: Plaud full raw + YouTube auto captions for the same Matt Pocock talk.
3. Primary score: **mean WER** across both references.
4. S/I/D from `jiwer.process_words` after normalization.
5. Still not a human gold set — for release claims, spot-check hard segments manually.

Machine data: `eval_dual_results.json`, `eval_dual_results.csv`.

## Aurum OpenRouter path (chunked 210s)

Client-side **4× ~3.5 min** chunks + stitch so the **Aurum CLI** remote path works past `max_segment_chars=8000`.

| Rank | Model | Status | Mean WER | vs Plaud | vs YouTube | Wall s |
|-----:|-------|--------|---------:|---------:|-----------:|-------:|
| 1 | `fish-audio/transcribe-1` | pass | 1.85% | 1.18% | 2.52% | 33.55 |
| 2 | `mistralai/voxtral-mini-transcribe` | pass | 1.92% | 1.36% | 2.47% | 22.98 |
| 3 | `microsoft/mai-transcribe-1.5` | pass | 1.98% | 1.71% | 2.25% | 21.25 |
| 4 | `openai/gpt-4o-transcribe` | pass | 2.05% | 1.58% | 2.52% | 48.9 |
| 5 | `qwen/qwen3-asr-flash-2026-02-10` | pass | 2.09% | 1.62% | 2.56% | 118.8 |
| 6 | `openai/whisper-1` | pass | 2.14% | 1.75% | 2.52% | 52.36 |
| 7 | `openai/gpt-4o-mini-transcribe` | pass | 2.44% | 1.93% | 2.96% | 34.35 |
| 8 | `openai/whisper-large-v3-turbo` | pass | 2.73% | 2.37% | 3.09% | 18.13 |
| 9 | `nvidia/parakeet-tdt-0.6b-v3` | pass | 3.37% | 2.98% | 3.75% | 18.27 |
| 10 | `deepgram/nova-3` | pass | 4.18% | 3.81% | 4.55% | 18.37 |
| 11 | `x-ai/grok-stt-1.0` | pass | 4.51% | 4.03% | 4.99% | 18.77 |
| 12 | `openai/whisper-large-v3` | pass | 14.61% | 14.20% | 15.01% | 24.3 |
| 13 | `google/chirp-3` | partial | 94.72% | 94.04% | 95.41% | 41.18 |

### Failures / partials

- `google/chirp-3` (partial): chunk0:rc=4:error: openrouter returned an error: HTTP 400 Bad Request: {"error":{"message":"Provider returned 400","code":400}}; chunk1:rc=4:error: openrouter returned an error: HT

**Headline findings**

1. **Chunking unblocks Aurum CLI OpenRouter** on this 11 min lecture (previously all failed at 8k segment cap).
2. **`gpt-4o-transcribe` is fine when chunked** (~2.05% mean WER) vs ~23% on unchunked full-file (deletions/length) — not a model quality gap.
3. **Parity with direct API** for healthy models (fish/voxtral/mai ~1.9–2.0%).
4. `openai/whisper-large-v3` degraded under chunking (14.6%) — first chunk short/truncated; worth re-check.
5. `google/chirp-3` still provider 400 on most chunks.
6. Offline **Aurum `medium.en` (1.87%)** still beats most remote chunked runs except fish/voxtral/mai.

Details: `CHUNKED_OPENROUTER_REPORT.md`.

## Human gold subset

Hard-segment kit ready for your ears (not auto-filled):

- Path: `gold_subset/` (also `~/Plaud/exports/matt-pocock-prototype-skill/gold_subset/`)
- **8 hard + 1 easy** ~50s clips with `audio.wav`, Plaud/YT drafts, empty `gold.txt`
- Annotate: listen → write verbatim into each `gold.txt` → `python3 gold_subset/score_against_gold.py`
- See `gold_subset/README.md` for the clip index and hardness scores.
