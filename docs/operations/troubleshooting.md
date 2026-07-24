# Troubleshooting

## ffmpeg not found

Install system ffmpeg and ensure it is on `PATH`.

```bash
brew install ffmpeg          # macOS
sudo apt install ffmpeg     # Debian/Ubuntu
winget install ffmpeg       # Windows
```

## First run is slow / large download

Use a quantized model:

```bash
aurum file.m4a --model tiny-q5_1
aurum models   # see cache status
```

## Empty transcript

Often silence or non-speech. Try `--language en` and `-v`. Special tokens like
`[BLANK_AUDIO]` are stripped by design.

## OpenRouter SRT refused

Expected. Use `-o json` or pass `--allow-unreliable-timestamps`.

## Metal / abort on process exit (library)

Call `aurum_core::providers::local::clear_context_cache()` before exit.

## Build fails on whisper-rs

Need **cmake** and a C++ compiler. On macOS, Xcode CLT + `brew install cmake`.
