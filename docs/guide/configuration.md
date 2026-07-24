# Configuration

## Precedence

1. **CLI flags**
2. **Environment** (OpenRouter only): `OPENROUTER_API_KEY`, `OPENROUTER_BASE_URL`
3. **Config file**
4. **Built-in defaults**

## Environment

| Variable | Purpose |
|----------|---------|
| `OPENROUTER_API_KEY` | Remote ASR / cleanup auth (preferred over file) |
| `OPENROUTER_BASE_URL` | Override API base (tests / proxies) |
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

[openrouter]
# api_key = "sk-or-..."    # prefer env var
# model = "google/gemini-2.5-flash-lite"
# base_url = "https://openrouter.ai/api/v1"
```

## Safety limits

| Limit | Default |
|-------|---------|
| Max duration | ~2.25 hours |
| Max decoded PCM | ~500 MB (enforced during decode) |
| Max remote upload | ~24 MB compressed |

Whisper special tokens such as `[BLANK_AUDIO]` are stripped. Segment timestamps
are clamped to audio duration.
