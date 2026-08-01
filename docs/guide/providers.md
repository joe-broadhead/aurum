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

## OpenRouter

| | |
|--|--|
| Auth | `OPENROUTER_API_KEY` (preferred) or config |
| Paths | Dedicated `/audio/transcriptions` **or** multimodal chat (`input_audio`) |
| Mode | `--openrouter-stt-mode auto\|chat\|transcriptions` (default `auto`) |
| SRT | Only when the route reports reliable timestamps (dedicated ASR) |

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
