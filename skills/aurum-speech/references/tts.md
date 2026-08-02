# TTS (text → speech)

## Contract

- CLI is **subcommand only**: `aurum tts …` (does not change STT defaults).
- Result is always **mono WAV** on disk (`-O` / `--output-file` required for synth).
- In-memory result is mono `i16` PCM; `--emit-json` honesty metadata never includes PCM.
- `backend_kind`: `"local"` or `"remote"`.

## Providers

| Provider | Auth env | Default model (if only `--provider`) | Voice notes |
|----------|----------|--------------------------------------|-------------|
| `local` | — | `kitten-nano-int8` | default voice `Luna`; opt-in `kokoro-82m-int8` + Kokoro voices |
| `openrouter` | `OPENROUTER_API_KEY` | `hexgrad/kokoro-82m` | remote voice ids (e.g. `alloy`); **not** Kitten aliases; tier **experimental** until protected smoke |
| `openai` | `OPENAI_API_KEY` | reviewed default (`tts-1` family) | e.g. `alloy`; first-party only |
| `elevenlabs` | `ELEVENLABS_API_KEY` | reviewed multilingual/turbo/flash | **required** ElevenLabs `voice_id` (never remap Luna) |
| `xai` | `XAI_API_KEY` | `xai-tts` | `eve` \| `ara` \| `leo` \| `rex` \| `sal` only; **experimental** |

When the user sets **only** `--provider` for remote TTS, Aurum uses that provider’s
reviewed model/voice defaults — **not** local Kitten/`Luna`.

## Local commands

```bash
aurum tts "Hello from aurum" -O /tmp/a.wav --force
aurum tts --input-file prompt.txt -O /tmp/a.wav --force
aurum tts "Hello" -O /tmp/a.wav --force --emit-json
aurum tts "Hello" -O /tmp/a.wav --voice Luna --speaking-rate 1.0 --force
# Higher quality local opt-in (~120 MB first download)
aurum tts "Hello from Kokoro" --model kokoro-82m-int8 --voice Heart -O /tmp/k.wav --force
aurum tts models
aurum tts voices
aurum tts adapters
```

- First local run may download the pinned pack (~26 MB Kitten).
- `--local-only` fails if pack not cached (no download).
- `--force` required to overwrite non-empty files.
- Speaking rate: finite, clamped about `0.5..=2.0` on CLI.
- `--cleanup raw|clean` is rules-only before synth (not full STT cleanup styles).

## Remote commands

```bash
# OpenRouter (default model hexgrad/kokoro-82m when only provider set)
export OPENROUTER_API_KEY=…
aurum tts "Hello" --provider openrouter -O /tmp/or.wav --force --emit-json
aurum tts "Hello" --provider openrouter --model fish-audio/s1 --voice alloy -O /tmp/or2.wav --force

# OpenAI
export OPENAI_API_KEY=…
aurum tts "Hello" --provider openai --model tts-1 --voice alloy -O /tmp/oai.wav --force

# ElevenLabs — explicit voice_id
export ELEVENLABS_API_KEY=…
aurum tts "Hello" --provider elevenlabs \
  --model eleven_multilingual_v2 --voice 21m00Tcm4TlvDq8ikWAM -O /tmp/el.wav --force

# xAI
export XAI_API_KEY=…
aurum tts "Hello" --provider xai --model xai-tts --voice eve -O /tmp/x.wav --force
```

Reviewed OpenRouter speech pins also include `sesame/csm-1b` and MP3-only
`minimax/speech-2.8-turbo` (explicit). Dead OpenAI-family OpenRouter ids are
**not** accepted — do not suggest `openai/gpt-4o-mini-tts*` on OpenRouter.

## Wire formats

Remote vendors may return PCM / WAV / MP3. Aurum normalizes through a bounded
pipeline to mono PCM then writes WAV. Agents should not promise raw vendor
container delivery to the user file.

## Demo WAVs (local only)

```bash
./scripts/generate_tts_demos.sh
```

Do **not** commit generated WAVs.

## FFI / library

- **Library:** `AurumEngine` + `tts_provider` / `synthesize` — can use remote when configured with secrets (Rust host only).
- **C ABI (`aurum-ffi`):** **local TTS jobs only** — do not claim remote TTS via FFI.
