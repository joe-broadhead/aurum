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

# An optional reviewed deployment catalogue. It can only narrow the built-in
# catalogue: an `enabled = false` entry disables a built-in STT model (by
# canonical id or alias) and needs no origin, and `[defaults.stt].global` picks
# the default from the remaining `supported` records (never experimental).
# Records that add or replace a model, and TTS records, are rejected for now.
# The path is explicit: Aurum never discovers, fetches, or falls back from it.
# A relative path resolves against this config file's directory (not the cwd).
# [catalogue]
# path = "/absolute/path/to/model-catalogue.toml"
```

Only canonical sections are accepted: `[stt]`, `[cleanup]`, `[tts]`, `[providers.*]`, `[catalogue]`.
Unknown top-level sections (including old `[default]` / `[openrouter]`) fail closed.

A catalogue can only set a single global default per direction, and that
default must be a local `supported` record. Experimental and explicit-only
records (such as the Portuguese specialists) are reachable through an explicit
`[stt].model` / `--model` only; `language` never selects a model. The
diagnostic catalogue digest covers the effective records and defaults, not the
deployment file location, so relocating an unchanged catalogue keeps the same
digest. Because a deployment can only narrow, every remaining record reports
`source = builtin`. The deployment's effect shows as `catalogue_disabled` (the
canonical ids it disabled) and as a changed digest.

#### Deployment catalogue trust model

`[catalogue].path` is a **trusted, operator-owned input**: it decides which
local speech-to-text models this installation may use.

- **Narrowing only.** A deployment catalogue can disable built-in STT models
  and choose the default STT model. A disabled model is rejected on every
  CLI and `AurumEngine` path: `--model`, `[stt].model`, `aurum batch`,
  `aurum converse`, `aurum cache repair` (whatever `[stt].provider` is),
  `AurumEngine` transcription and preload, and the raw
  `AurumEngine::local_whisper()` / `stt_provider(local)` accessors, whose
  provider carries the policy into its own `preload` and `transcribe`. It is
  also hidden from `aurum models` and never recommended: `aurum models
  recommend` and `--profile` fall back to the profile's first enabled
  alternative, and fail when none is enabled. `aurum models` only names a
  model the catalogue still allows, in the table and in its closing guidance.
  `aurum cache status` / `verify` still check a disabled model's file on disk,
  mark it as disabled, and never re-download it. `aurum doctor` reports a
  configured model that the catalogue disables. A rejected id says it was
  disabled by the deployment catalogue (`[catalogue].path`); only a name the
  catalogue never contained is reported as an unknown model.
- **Where the default applies.** When no `[stt].model` is set, every CLI path
  and every `AurumEngine` request that resolves a model through `Config` uses
  the deployment's `[defaults.stt].global`, as does a projected
  `aurum_core::AurumConfig`. A library host that constructs
  `TranscriptionOptions::default()` still asks for the built-in `base`: that
  default cannot read a config, and a deployment that disables `base` therefore
  rejects the request instead of silently choosing another model. Pass
  `config.default_local_stt_model()` (or an explicit model) in that case.
- **Not covered: `aurum-ffi` engines.** C/FFI hosts build their engine without
  a config file, so `[catalogue].path` does not bind FFI transcription, jobs or
  preload yet. An FFI host that must honour the policy has to enforce it
  itself. `aurum_doctor_json` is the exception: it calls `Config::load()`, so
  its report does reflect the configured catalogue. An opt-in engine option for
  the rest is tracked in
  [the deployment records issue](https://github.com/joe-broadhead/aurum/issues/146).
  Model downloads, cache pins and TTS selection still come from the built-in
  catalogue, so a deployment record that adds or replaces a model, or any TTS
  record, fails closed. Supporting those records is tracked in the same issue.
- **Disable entries.** A disable entry names a built-in STT model and keeps the
  schema's required fields (`id`, `direction`, `provider`, `tier`), but needs no
  `origin`: declaring one is an error, because those pins would never be fetched
  or compared with the built-in pins.

  ```toml
  schema_version = 1

  [defaults.stt]
  global = "small"

  [[model]]
  id = "base"
  direction = "stt"
  provider = "local"
  tier = "supported"
  enabled = false

  [[model]]
  id = "turbo"          # alias of `large-v3-turbo`
  direction = "stt"
  provider = "local"
  tier = "supported"
  enabled = false
  ```

  The entry matches a built-in STT model by id or alias and removes it. An
  entry that names a TTS model is rejected. Disabling a model and one of its
  aliases in the same file is accepted.
- **Path resolution.** A relative path resolves against the directory of the
  config file that names it. It never resolves against the process working
  directory, and it is an error when no config file is in use. An absolute
  path is recommended.
- **File policy.** The target must be a regular file of at most 1 MiB. A
  symlink is refused, not followed. Aurum reads the file once at startup and
  fails closed on any read, size, schema, or validation error. There is no
  environment-variable, discovery, or built-in fallback.
- **Directory scope.** Only the final path component is checked for a
  symlink; parent directories are followed. On Unix, Aurum also checks that
  the opened file is the one it inspected (same device and inode); Windows
  has no such check. Keep the catalogue in a directory that only the operator
  can write to.

The embedded catalogue is held to a stricter rule. Its URLs may only use the
reviewed hosts, and every Hugging Face URL must name an immutable revision.
GitHub is allowed only for an explicitly reviewed release, which today is
`thewh1teagle/kokoro-onnx` tag `model-files-v1.0` (the Kokoro ONNX model and
voices). Release tags can be moved, so for those assets the exact size and
SHA-256 pins are the identity. Both are enforced when the file is downloaded
and every time the cache is verified, so a replaced or truncated asset fails
closed rather than being used.

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
