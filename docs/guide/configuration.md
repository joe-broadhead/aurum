# Configuration

## Precedence

1. **CLI flags**
2. **Environment**: provider secrets (`OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ELEVENLABS_API_KEY`, `XAI_API_KEY`), OpenRouter base URL, and TTS (`AURUM_TTS_*`)
3. **Config file**
4. **Built-in defaults**

A provider is **never** selected merely because its API key is present. Omitted STT/TTS `provider` remains `local`.

## Environment

| Variable | Purpose |
|----------|---------|
| `OPENROUTER_API_KEY` | OpenRouter auth (preferred over file `api_key`) |
| `OPENROUTER_BASE_URL` | Override OpenRouter API base (tests / proxies) |
| `OPENAI_API_KEY` | OpenAI provider-scoped secret |
| `ELEVENLABS_API_KEY` | ElevenLabs provider-scoped secret |
| `XAI_API_KEY` | xAI provider-scoped secret |
| `AURUM_TTS_MODEL` | Override `[tts].model` |
| `AURUM_TTS_VOICE` | Override `[tts].voice` |
| `AURUM_TTS_LANGUAGE` | Override `[tts].language` |
| `RUST_LOG` | Tracing filters |

Secrets are stored as redacting `SecretString` values. Effective-config / doctor / support output shows **presence only** (`***`), never plaintext.

## Config file

Resolved via the `directories` crate (app name `aurum`):

| Platform | Typical path |
|----------|----------------|
| macOS | `~/Library/Application Support/aurum/config.toml` |
| Linux | `~/.config/aurum/config.toml` |
| Windows | `%APPDATA%\aurum\config.toml` |

### Canonical schema

```toml
[stt]
provider = "local"          # local | openrouter | openai | xai
model = "base"
language = "auto"
# output = "txt"            # txt | srt | json

[tts]
provider = "local"          # local | openrouter | openai | elevenlabs | xai
model = "kitten-nano-int8"
voice = "Luna"
language = "en"
speaking_rate = 1.0
max_chars = 5000
timeout_ms = 120000
# Optional local pack override (directory with aurum-tts-manifest.json — not a bare .onnx)
# pack_dir = "/path/to/pack"
# allow_unverified = false

# Optional custom catalogue entries (JOE-1620). Never shadow built-in ids.
# [[tts.custom_models]]
# id = "my-tone"
# adapter = "fake-sine-v1"
# pack_dir = "/path/to/pack"
# trust = "verified"   # verified | local_unverified (never builtin)
# license = "CC0"

[cleanup]
style = "raw"              # raw | clean | bullets | professional | summary
provider = "rules"         # rules | openrouter
# openrouter_model = "google/gemini-2.5-flash-lite"

# Named provider options + optional file secrets (prefer env vars for keys).
# Unknown [providers.*] keys fail closed.

# [providers.openrouter]
# stt_mode = "auto"          # auto | chat | transcriptions
# model = "google/gemini-2.5-flash"
# base_url = "https://openrouter.ai/api/v1"
# allow_custom_endpoint = false
# use_system_proxy = false

# [providers.openai]
# base_url = "https://api.openai.com/v1"

# [providers.elevenlabs]
# [providers.xai]
```

Only canonical sections are accepted: `[stt]`, `[cleanup]`, `[tts]`, `[providers.*]`.
Unknown top-level sections (including old `[default]` / `[openrouter]`) fail closed.

### `local_only`

When `local_only` is set on the runtime/validated config (CLI offline flag and library builders), validation rejects a remote STT or TTS provider **before** encoding, upload, or request construction.

## Safety limits

| Limit | Default |
|-------|---------|
| Max duration (STT) | ~2.25 hours |
| Max decoded PCM (STT) | ~500 MB (enforced during decode) |
| Max remote upload (STT) | ~24 MB compressed |
| Max TTS characters | 5000 (`[tts].max_chars`) |
| TTS timeout | 120000 ms (`[tts].timeout_ms`) |
| TTS speaking rate | 1.0, allowed range `0.5..=2.0` (CLI clamps the same) |

Whisper special tokens such as `[BLANK_AUDIO]` are stripped. Segment timestamps
are clamped to audio duration.
