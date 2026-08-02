# Troubleshooting

## ffmpeg not found

```bash
brew install ffmpeg          # macOS
sudo apt install ffmpeg      # Debian/Ubuntu
winget install ffmpeg        # Windows
```

## First run is slow / large download

```bash
aurum file.m4a --model tiny-q5_1   # ~32 MB
aurum models                       # cache status
```

## Empty transcript

Often silence or non-speech. Try `--language en` and `-v`. Special tokens like
`[BLANK_AUDIO]` are stripped by design.

```bash
aurum tests/fixtures/silence.wav --model tiny -o json
```

## OpenRouter errors

| Message | Meaning |
|---------|---------|
| API key is missing | Set `OPENROUTER_API_KEY` |
| No endpoints available matching your guardrail restrictions | Account privacy/data policy blocks the model — fix https://openrouter.ai/settings/privacy |
| No endpoints found that support input audio | Model is text-only — pick a reviewed audio-capable id |
| looks like a local whisper model | Don’t pass `tiny`/`base` with `--provider openrouter` |
| SRT refused | Use `-o json` or `--allow-unreliable-timestamps` |
| model not in reviewed registry | Use a listed id, or set `--openrouter-stt-mode chat` / `transcriptions` explicitly |

## OpenRouter SRT refused

Expected for **llm_assisted** (chat multimodal) routes. Prefer dedicated ASR models
from the reviewed registry for SRT, or use `-o txt` / `-o json`.

## Remote STT: segment too long / truncated long files

First-party OpenAI/xAI paths reject a single segment when transcript text exceeds
the max segment bound (~8k characters). Very long lectures may need external
chunking until automatic chunk-and-stitch lands (tracked for v0.0.21). Prefer
local whisper for full long-form offline, or shorter remote segments.

## Other remote providers

| Provider | Typical issues |
|----------|----------------|
| `openai` | Missing `OPENAI_API_KEY`; use reviewed STT/TTS model ids |
| `elevenlabs` | TTS only; requires explicit `voice_id` (not Kitten aliases) |
| `xai` | Experimental; official `/v1/stt` + `/v1/tts` only; voices `eve\|ara\|leo\|rex\|sal` |

## Remote STT long-form (JOE-2212)

OpenAI, OpenRouter, and xAI automatically **time-chunk** audio longer than ~210s
and stitch text/segments with offsets. Short files stay single-request.

- Override window: `AURUM_REMOTE_STT_CHUNK_SECS` (positive seconds)
- Cancel is checked between chunks
- Local whisper is unchanged (full-file offline)

## Metal / abort on process exit (library)

```rust
aurum_core::providers::local::clear_context_cache();
// or from C / aurum-ffi:
// aurum_shutdown();
```

Call before process exit on macOS.

## Build fails on whisper-rs

Need **cmake** and a C++ compiler. On macOS: Xcode CLT + `brew install cmake`.

## TTS: pack missing / offline

```bash
aurum tts models          # cache status
# First run needs network once to download the pinned pack (~26 MB), or:
aurum tts "Hello" -O /tmp/a.wav --local-only   # fails closed if not cached
```

## TTS: timeout

`--timeout` is a **wait** bound on the synthesis worker. ONNX work may continue briefly after the error returns (best-effort cancel). Raise `[tts].timeout_ms` for long passages.

## TTS: overwrite refused

```bash
aurum tts "Hi" -O out.wav --force
```
