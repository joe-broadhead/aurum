# aurum-core overview

`aurum-core` is the reusable library behind the CLI. It is designed so apps like
**ZephyrFlow** can share the same transcription stack without shelling out.

## What you get

- `TranscriptionProvider` trait
- `LocalWhisperProvider` (whisper.cpp)
- `OpenRouterProvider` (LLM-assisted)
- Audio load/convert with safety limits
- Model download + cache + integrity checks
- Output formatters (`txt` / `srt` / `json`)
- Post-processing (special-token strip, timestamp clamp)

## Stability

!!! warning "Experimental API"
    Until `0.1.0`, expect breaking changes. Pin a git revision in production consumers.

## Crates.io

Not published yet. Consume via git/path dependency (see [Integration](integration.md)).
