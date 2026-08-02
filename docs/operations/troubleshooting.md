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
| guardrail restrictions / data policy | **Privacy prerequisite:** https://openrouter.ai/settings/privacy |
| No endpoints found that support input audio | Model is text-only — pick an audio-capable id |
| looks like a local whisper model | Don’t pass `tiny`/`base` with `--provider openrouter` |
| SRT refused | Use `-o json` or `--allow-unreliable-timestamps` |

OpenRouter account privacy/guardrails must allow the model, or **all** endpoints
can fail. Re-probe catalogues after policy changes:
`./scripts/probe_provider_catalogues.sh --live` (JOE-2213).

## OpenRouter SRT refused

Expected. LLM timestamps are unreliable. Prefer `-o txt` or `-o json`.

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
