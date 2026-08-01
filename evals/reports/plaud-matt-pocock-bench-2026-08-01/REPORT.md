# Plaud vs Aurum / OpenRouter transcription bench

- **generated_at_utc:** 2026-08-01T18:44:03Z
- **source:** Plaud recording `42e86284875bf900d98cb7e83ce3b7cb` — *07-30 Lecture: AI-Driven Code Generation and Prototyping*
- **audio:** 685.1s MP3 (10962276 bytes)
- **reference:** Plaud transcript (speaker labels/timestamps stripped) — **2281 words**
- **aurum:** aurum 0.0.17 local whisper.cpp path
- **openrouter path:** direct `POST /api/v1/audio/transcriptions` (Aurum CLI rejected full-length single-segment responses at 8 000 chars/segment — product limit)

> WER/CER measure **agreement with Plaud**, not absolute human ground truth. Plaud may already use commercial ASR + cleanup.

## Leaderboard (best WER first)

| Rank | System | Model | Status | WER ↓ | CER | Wall s | RTF |
|-----:|--------|-------|--------|------:|----:|-------:|----:|
| 1 | openrouter-direct | `fish-audio/transcribe-1` | pass | 1.23% | 0.64% | 18.89 | 0.0276 |
| 2 | openrouter-direct | `mistralai/voxtral-mini-transcribe` | pass | 1.36% | 0.80% | 7.5 | 0.0110 |
| 3 | local | `medium.en` | pass | 1.45% | 0.85% | 82.67 | 0.1207 |
| 4 | local | `large-v3-turbo` | pass | 1.53% | 0.95% | 61.87 | 0.0903 |
| 5 | openrouter-direct | `microsoft/mai-transcribe-1.5` | pass | 1.58% | 0.90% | 4.88 | 0.0071 |
| 6 | local | `small.en` | pass | 1.71% | 1.20% | 31.2 | 0.0455 |
| 7 | openrouter-direct | `openai/whisper-1` | pass | 1.93% | 1.21% | 40.27 | 0.0588 |
| 8 | openrouter-direct | `openai/whisper-large-v3-turbo` | pass | 2.54% | 1.45% | 2.19 | 0.0032 |
| 9 | local | `small` | pass | 2.67% | 1.70% | 30.85 | 0.0450 |
| 10 | openrouter-direct | `nvidia/parakeet-tdt-0.6b-v3` | pass | 2.81% | 1.55% | 2.83 | 0.0041 |
| 11 | openrouter-direct | `deepgram/nova-3` | pass | 2.89% | 1.90% | 2.21 | 0.0032 |
| 12 | local | `large-v3-turbo-q5_0` | pass | 2.94% | 1.99% | 56.07 | 0.0818 |
| 13 | openrouter-direct | `openai/whisper-large-v3` | pass | 4.08% | 3.16% | 3.38 | 0.0049 |
| 14 | local | `base.en` | pass | 4.34% | 2.59% | 13.47 | 0.0197 |
| 15 | local | `base` | pass | 5.74% | 3.26% | 12.17 | 0.0178 |
| 16 | openrouter-direct | `x-ai/grok-stt-1.0` | pass | 6.31% | 4.83% | 4.23 | 0.0062 |
| 17 | local | `tiny-q5_1` | pass | 10.13% | 5.80% | 7.69 | 0.0112 |
| 18 | openrouter-direct | `openai/gpt-4o-mini-transcribe` | pass | 22.93% | 22.54% | 16.02 | 0.0234 |
| 19 | openrouter-direct | `openai/gpt-4o-transcribe` | pass | 23.19% | 22.87% | 25.85 | 0.0377 |
| 20 | local | `large-v3` | fail | — | — | 6.77 | 0.0099 |
| 21 | openrouter-direct | `google/chirp-3` | fail | — | — | 44.52 | 0.0650 |
| 22 | openrouter-direct | `qwen/qwen3-asr-flash-2026-02-10` | fail | — | — | 3.83 | 0.0056 |

## Aurum local (whisper.cpp)

- **`medium.en`** — WER 1.45%, CER 0.85%, 82.67s (RTF 0.1207)
- **`large-v3-turbo`** — WER 1.53%, CER 0.95%, 61.87s (RTF 0.0903)
- **`small.en`** — WER 1.71%, CER 1.20%, 31.2s (RTF 0.0455)
- **`small`** — WER 2.67%, CER 1.70%, 30.85s (RTF 0.045)
- **`large-v3-turbo-q5_0`** — WER 2.94%, CER 1.99%, 56.07s (RTF 0.0818)
- **`base.en`** — WER 4.34%, CER 2.59%, 13.47s (RTF 0.0197)
- **`base`** — WER 5.74%, CER 3.26%, 12.17s (RTF 0.0178)
- **`tiny-q5_1`** — WER 10.13%, CER 5.80%, 7.69s (RTF 0.0112)
- **`large-v3`** — FAIL: error: resource overload: model weight 4642550224 exceeds residency budget 3221225472

## OpenRouter `output_modalities=transcription`

- **`fish-audio/transcribe-1`** — WER 1.23%, 18.89s
- **`mistralai/voxtral-mini-transcribe`** — WER 1.36%, 7.5s
- **`microsoft/mai-transcribe-1.5`** — WER 1.58%, 4.88s
- **`openai/whisper-1`** — WER 1.93%, 40.27s
- **`openai/whisper-large-v3-turbo`** — WER 2.54%, 2.19s
- **`nvidia/parakeet-tdt-0.6b-v3`** — WER 2.81%, 2.83s
- **`deepgram/nova-3`** — WER 2.89%, 2.21s
- **`openai/whisper-large-v3`** — WER 4.08%, 3.38s
- **`x-ai/grok-stt-1.0`** — WER 6.31%, 4.23s
- **`openai/gpt-4o-mini-transcribe`** — WER 22.93%, 16.02s
- **`openai/gpt-4o-transcribe`** — WER 23.19%, 25.85s
- **`google/chirp-3`** — FAIL: {"error":{"message":"Provider returned 400","code":400}}
- **`qwen/qwen3-asr-flash-2026-02-10`** — FAIL: {"error":{"message":"Provider returned 400","code":400}}

## Notes

- Local `large-v3` (full) failed residency budget: weight ~4.3 GiB > default max ~3.0 GiB.
- Aurum CLI OpenRouter path returned the remote text but **failed closed** on `max_segment_chars=8000` for this ~11 min lecture (single segment).
- Direct OpenRouter API used for fair quality comparison of the 13 transcription models.
- Hypotheses under `hypotheses/`; row data in `results.jsonl` + `results_openrouter.jsonl`.

## Extra observations

- **Best offline (Aurum):** `medium.en` (WER 1.45%, ~83s) and `large-v3-turbo` (1.53%, ~62s). `small.en` is the speed/quality sweet spot (1.71% in ~31s).
- **Best cloud:** `fish-audio/transcribe-1` (1.23%) slightly beats local medium.en; wording matches Plaud closely (e.g. "write code" vs many models' "create code") — Plaud may share stack/post-processing DNA with commercial ASR.
- **gpt-4o(-mini)-transcribe** high WER (~23%) is largely **length**: ~1.8k words vs ~2.3k ref — content dropped / paraphrased, not pure substitution noise.
- **Fails:** `google/chirp-3`, `qwen/qwen3-asr-flash-2026-02-10` (provider 400 on this file); local `large-v3` residency budget.
- **Aurum product note:** CLI OpenRouter path fails closed on long single-segment transcripts (`max_segment_chars=8000`); long lectures need chunking or a limit raise for remote path.
