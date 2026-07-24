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
| guardrail restrictions / data policy | Fix https://openrouter.ai/settings/privacy |
| No endpoints found that support input audio | Model is text-only — pick an audio-capable id |
| looks like a local whisper model | Don’t pass `tiny`/`base` with `--provider openrouter` |
| SRT refused | Use `-o json` or `--allow-unreliable-timestamps` |

## OpenRouter SRT refused

Expected. LLM timestamps are unreliable. Prefer `-o txt` or `-o json`.

## Metal / abort on process exit (library)

```rust
aurum_core::providers::local::clear_context_cache();
```

Call before process exit on macOS.

## Build fails on whisper-rs

Need **cmake** and a C++ compiler. On macOS: Xcode CLT + `brew install cmake`.
