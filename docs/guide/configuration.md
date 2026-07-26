# Configuration

## Precedence

1. **CLI flags**
2. **Environment**: OpenRouter (`OPENROUTER_API_KEY`, `OPENROUTER_BASE_URL`) and TTS (`AURUM_TTS_*`)
3. **Config file**
4. **Built-in defaults**

## Environment

| Variable | Purpose |
|----------|---------|
| `OPENROUTER_API_KEY` | Remote ASR / cleanup auth (preferred over file) |
| `OPENROUTER_BASE_URL` | Override API base (tests / proxies) |
| `AURUM_TTS_MODEL` | Override `[tts].model` |
| `AURUM_TTS_VOICE` | Override `[tts].voice` |
| `AURUM_TTS_LANGUAGE` | Override `[tts].language` |
| `RUST_LOG` | Tracing filters |

## Config file

Resolved via the `directories` crate (app name `aurum`):

| Platform | Typical path |
|----------|----------------|
| macOS | `~/Library/Application Support/aurum/config.toml` |
| Linux | `~/.config/aurum/config.toml` |
| Windows | `%APPDATA%\aurum\config.toml` |

```toml
[default]
provider = "local"
model = "base"
language = "auto"
output = "txt"

[cleanup]
style = "raw"              # raw | clean | bullets | professional | summary
provider = "rules"         # rules | openrouter
# openrouter_model = "google/gemini-2.5-flash-lite"

[tts]
provider = "local"
model = "kitten-nano-int8"
voice = "Luna"
language = "en"
max_chars = 5000
timeout_ms = 120000

[openrouter]
# api_key = "sk-or-..."    # prefer env var
# model = "google/gemini-2.5-flash-lite"
# base_url = "https://openrouter.ai/api/v1"
```

## Safety limits

| Limit | Default |
|-------|---------|
| Max duration (STT) | ~2.25 hours |
| Max decoded PCM (STT) | ~500 MB (enforced during decode) |
| Max remote upload (STT) | ~24 MB compressed |
| Max TTS characters | 5000 (`[tts].max_chars`) |
| TTS timeout | 120000 ms (`[tts].timeout_ms`) |

Whisper special tokens such as `[BLANK_AUDIO]` are stripped. Segment timestamps
are clamped to audio duration.
