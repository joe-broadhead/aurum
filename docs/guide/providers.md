# Providers

ASR backends implement `TranscriptionProvider`. Cleanup is a **separate** stage
(see [Cleanup](cleanup.md)).

## Local (default)

| | |
|--|--|
| Engine | whisper.cpp via `whisper-rs` |
| Auth | None |
| GPU | Metal on macOS builds |
| Models | ggml catalogue — `aurum models` |
| Context | Process-level cache; call `clear_context_cache()` before exit in long-lived hosts |
| Integrity | Magic check + pinned SHA-256 for common models |

```bash
aurum talk.m4a --model tiny-q5_1
aurum talk.m4a --model base
```

## OpenAI (first-party, JOE-1940)

| | |
|--|--|
| Auth | `OPENAI_API_KEY` or `[providers.openai]` |
| STT | Multipart `POST /audio/transcriptions` (official origin only) |
| TTS | `POST /audio/speech` (prefer PCM) |
| STT models | Reviewed: `whisper-1`, `gpt-4o-mini-transcribe`, `gpt-4o-transcribe` |
| TTS models | Reviewed: `tts-1`, `tts-1-hd`, `gpt-4o-mini-tts` |

```bash
export OPENAI_API_KEY=sk-...
aurum talk.mp3 --provider openai --model whisper-1
aurum tts "Hello" --provider openai --model tts-1 --voice alloy -O /tmp/oai.wav
```

Independent from OpenRouter: different credential, origin (`api.openai.com`), and
no OpenRouter attribution headers. Audio/text leave the machine only with
explicit `provider=openai`.

## OpenRouter

| | |
|--|--|
| Auth | `OPENROUTER_API_KEY` (preferred) or config |
| STT paths | Dedicated `/audio/transcriptions` **or** multimodal chat (`input_audio`) |
| TTS path | Dedicated `/audio/speech` (OpenAI-compatible; JOE-1939) |
| Mode | `--openrouter-stt-mode auto\|chat\|transcriptions` (default `auto`) |
| SRT | Only when the route reports reliable timestamps (dedicated ASR) |

### Remote TTS (opt-in)

```bash
export OPENROUTER_API_KEY=sk-or-...
aurum tts "Hello from OpenRouter" --provider openrouter \
  --model openai/gpt-4o-mini-tts --voice alloy -O /tmp/or.wav --emit-json
```

- Only **reviewed** OpenRouter TTS models are accepted (fail closed).
- Voices are provider voice ids (e.g. `alloy`); local Kitten aliases are **not** remapped.
- Prefer PCM wire format; Aurum normalizes to mono `i16` via the shared remote-audio pipeline.
- **Privacy:** synthesis text is sent to OpenRouter (and potentially an upstream model).

### Capability-authoritative `auto` routing

`auto` does **not** guess from model-name substrings. It only routes models
present in the reviewed static registry (`OPENROUTER_STT_REGISTRY` in
`aurum-core`). Unknown model ids **fail closed** with an explicit error — set
`--openrouter-stt-mode chat` or `transcriptions` yourself for unlisted models.

| Registry class | HTTP path | `backend_kind` | Timestamps |
|----------------|-----------|----------------|------------|
| Dedicated ASR (e.g. `openai/whisper-large-v3`, `openai/gpt-4o-transcribe`) | `/audio/transcriptions` | `asr` | reliable |
| Multimodal chat (e.g. `google/gemini-2.5-flash`) | `/chat/completions` | `llm_assisted` | unreliable |

```bash
export OPENROUTER_API_KEY=sk-or-...
# Registered multimodal chat model (auto → chat)
aurum talk.mp3 --provider openrouter --model google/gemini-2.5-flash
# Registered dedicated ASR (auto → transcriptions)
aurum talk.mp3 --provider openrouter --model openai/whisper-large-v3 -o srt
# Unregistered model: must choose a path explicitly
aurum talk.mp3 --provider openrouter --model vendor/custom-audio \
  --openrouter-stt-mode chat -o json
```

!!! note "Privacy settings"
    OpenRouter account privacy/guardrails must allow the chosen provider, or you
    will see `No endpoints available matching your guardrail restrictions`.
    Configure at https://openrouter.ai/settings/privacy

```mermaid
flowchart TB
  subgraph local [local]
    A[PCM 16 kHz mono] --> B[WhisperContext cache]
    B --> C[segments + text]
  end
  subgraph remote [openrouter]
    D[encode upload] --> R{auto registry}
    R -->|ASR record| E1[/audio/transcriptions]
    R -->|chat record| E2[/chat/completions]
    R -->|unknown| X[fail closed]
    E1 --> F[text + timestamps]
    E2 --> G[text LLM-assisted]
  end
```
