# Quickstart

## First transcript

```bash
# List models + cache status
aurum models

# Fast trial (~32 MB quantized download on first use)
aurum path/to/audio.m4a --model tiny-q5_1

# Default local model is `base` (~142 MB)
aurum path/to/audio.m4a

# Subtitles
aurum path/to/audio.m4a --model tiny-q5_1 -o srt --output-file out.srt

# JSON (includes backend_kind + timestamps_reliable)
aurum path/to/audio.m4a -o json
```

## Sample fixture (from repo)

```bash
cargo run -p aurum -- tests/fixtures/sample.wav --model tiny-q5_1
```

## OpenRouter (optional)

```bash
export OPENROUTER_API_KEY=sk-or-...
aurum talk.mp3 --provider openrouter
# SRT is refused by default — LLM timestamps are unreliable
aurum talk.mp3 --provider openrouter -o json
```

!!! warning "LLM-assisted, not dedicated ASR"
    OpenRouter uses multimodal chat completions. Prefer local when verbatim accuracy matters.

## Config file

Platform config dir + `/aurum/config.toml`  
(e.g. `~/Library/Application Support/aurum/config.toml` on macOS).

```toml
[default]
provider = "local"
model = "base"
language = "auto"
output = "txt"
```

Environment variables override the file (`OPENROUTER_API_KEY`, `OPENROUTER_BASE_URL`).
