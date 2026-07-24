# Architecture

```text
aurum-stt (CLI)              aurum-ffi (C / embedders)
         \                      /
          \                    /
           ▼                  ▼
                 aurum-core
  audio/        file load + limits + from_pcm
  pcm/          mic buffer helpers (Rust hosts)
  window/       partial-window policy (host-driven)
  cancel/       CancelFlag
  model/        catalogue, download, lock, integrity
  providers/    local whisper.cpp · openrouter
  postprocess/  ASR markers, clamp, NaN guard
  cleanup/      rules | openrouter LLM
  output/       txt · srt · json
  config/
  error/        user | environment | provider
```

## Pipeline

```mermaid
flowchart LR
  A[File or PCM] --> B[AudioInput]
  B --> C[TranscriptionProvider]
  C --> D[postprocess]
  D --> E[optional TextCleanup]
  E --> F[format_result]
```

## Design rules

1. **CLI owns UX** — progress, first-run tips, OpenRouter SRT policy
2. **Core owns truth** — providers return normalized results + honesty fields
3. **FFI owns a narrow embed surface** — PCM, preload, cancel, rules cleanup (`aurum-ffi`)
4. **Fail closed** — bad magic, oversized audio, missing keys, offline missing models
5. **No default network** except explicit model download or remote provider
6. **ASR ≠ cleanup** — separate stages (transcription vs text cleanup)

## Process model cache

`LocalWhisperProvider` keeps a process-global `WhisperContext` map keyed by model
path. Drop contexts via `clear_context_cache()` (or `aurum_shutdown` from FFI)
before process exit so Metal/ggml teardown does not assert.
