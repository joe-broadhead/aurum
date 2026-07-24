# Architecture

```text
aurum (CLI)
 └── aurum-core
      ├── audio/          load + limits + upload encode
      ├── model/          catalogue, download, lock, integrity
      ├── providers/
      │    ├── local      whisper.cpp + context cache
      │    └── openrouter multimodal chat
      ├── postprocess/    markers, clamp, NaN guard
      ├── output/         txt srt json
      ├── config/
      └── error/          user | environment | provider
```

## Design rules

1. **CLI owns UX** — progress, first-run tips, SRT policy for OpenRouter  
2. **Core owns truth** — providers return `TranscriptionResult` with `backend_kind`  
3. **Fail closed** — bad magic, oversized audio, missing keys  
4. **No default network** except explicit model download or remote provider  

## Process model cache

`LocalWhisperProvider` keeps a process-global `WhisperContext` map keyed by model
path. Drop contexts via `clear_context_cache()` before exit so Metal/ggml teardown
does not assert.
