# STT (speech → text)

## Providers

| Provider | CLI | Auth env | Reviewed models (examples) | Notes |
|----------|-----|----------|----------------------------|--------|
| `local` | default | — | whisper catalogue (`tiny-q5_1`, `base`, …) via `aurum models` | Metal on macOS; offline after cache |
| `openrouter` | `--provider openrouter` | `OPENROUTER_API_KEY` | dedicated ASR e.g. `openai/whisper-large-v3`, `openai/gpt-4o-transcribe`; chat multimodal e.g. `google/gemini-2.5-flash` | `auto` routes **only** reviewed registry ids |
| `openai` | `--provider openai` | `OPENAI_API_KEY` | `whisper-1`, `gpt-4o-mini-transcribe`, `gpt-4o-transcribe` | First-party `api.openai.com` only |
| `xai` (`grok` alias) | `--provider xai` | `XAI_API_KEY` | `xai-stt` | Official `POST /v1/stt`; **experimental** |
| `elevenlabs` | — | — | — | **No STT** in Aurum |

## Local commands

```bash
aurum meeting.m4a
aurum meeting.m4a --model tiny-q5_1
aurum meeting.m4a --profile speed          # opt-in; default remains base without --profile
aurum meeting.m4a -o srt --output-file out.srt
aurum meeting.m4a -o json
aurum meeting.m4a --cleanup clean
aurum meeting.m4a --language en -v
```

- Default model: **`base`** (product default). Profiles do not change the default unless passed.
- `--model` always wins over `--profile`.
- **ffmpeg** required for non–16 kHz mono WAV (mp3, m4a, …).
- Integrity-pinned catalogue downloads; fail closed on mismatch.

## Remote commands

```bash
# OpenAI
aurum talk.mp3 --provider openai --model whisper-1
aurum talk.mp3 --provider openai --model gpt-4o-transcribe -o json

# OpenRouter — prefer reviewed dedicated ASR for SRT
aurum talk.mp3 --provider openrouter --model openai/whisper-large-v3 -o srt
# Multimodal chat (llm_assisted) — timestamps often unreliable
aurum talk.mp3 --provider openrouter --model google/gemini-2.5-flash -o json
# Force path for unlisted models only
aurum talk.mp3 --provider openrouter --model vendor/custom \
  --openrouter-stt-mode chat -o json

# xAI
aurum talk.mp3 --provider xai --model xai-stt
```

### OpenRouter STT modes

| Mode | When |
|------|------|
| `auto` (default) | Only if model is in reviewed static registry |
| `transcriptions` | Dedicated ASR HTTP path |
| `chat` | Multimodal chat / `input_audio` |

Unknown models with `auto` **fail closed** — set mode explicitly; do not invent registry entries.

### Timestamps / SRT

- Local whisper: timestamps OK for SRT.
- OpenRouter dedicated ASR: timestamps when route reports reliability.
- Chat multimodal / many remote routes: SRT **refused** unless `--allow-unreliable-timestamps`.
- Prefer `-o txt` or `-o json` when unsure.

### Long-form remote

Very long lectures can hit remote segment limits (~8k chars) or truncate on some
full-file paths. Prefer:

1. Local whisper for full long-form offline, or
2. Shorter chunks until automatic chunk-and-stitch ships (v0.0.21 track),

and say so honestly if the user hits errors.

## Cleanup after STT

Cleanup is a **separate stage** (not a provider):

```bash
aurum talk.m4a --cleanup clean
aurum talk.m4a --cleanup bullets --cleanup-provider rules
aurum talk.m4a --cleanup professional --cleanup-provider openrouter
echo "um, hello" | aurum cleanup -s clean
```

| Style | Intent |
|-------|--------|
| `raw` | off |
| `clean` | fillers / cleanup |
| `bullets` | bulletize |
| `professional` | formal tone |
| `summary` | short summary |

`--cleanup-provider`: `rules` (default, on-device) | `openrouter` (needs key).

## Batch STT

```bash
aurum batch ./lectures -O ./out --dry-run
aurum batch ./lectures -O ./out --model tiny-q5_1
aurum batch ./lectures -O ./out --resume --retry-failed
```

Use native `batch` (not a shell loop). See `skills/aurum-batch/`.

## Exit codes (shared)

| Code | Meaning |
|------|---------|
| 0 | success |
| 2 | user error (bad path, missing key, bad model) |
| 3 | environment (ffmpeg, I/O) |
| 4 | provider (network, inference, download) |
| 1 | internal |
