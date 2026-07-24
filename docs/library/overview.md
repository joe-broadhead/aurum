# aurum-core overview

`aurum-core` is the reusable library behind the CLI. Apps can share the same
transcription and cleanup stack without shelling out to the binary.

## What you get

| Area | API |
|------|-----|
| ASR | `TranscriptionProvider`, `LocalWhisperProvider`, `OpenRouterProvider` |
| Audio | `load_audio`, `AudioInput::from_pcm`, safety limits |
| PCM / mic hosts | `PcmBuffer`, `transcribe_pcm`, `preload`, `local_only` |
| Partials (host-driven) | `PartialWindowPolicy`, `PartialClock` |
| Cancel | `CancelFlag` in `TranscriptionOptions` |
| Models | catalogue, download, pins, progress callbacks |
| Cleanup | `RulesCleanup`, `OpenRouterCleanup`, `apply_cleanup*` |
| Output | `format_result` — txt / srt / json |
| Post-ASR | special-token strip, timestamp clamp |

## Stability

!!! warning "Experimental API"
    Until `0.1.0`, expect breaking changes. Pin a git **rev** or release **tag**
    in production consumers.

## Crates.io

Not published yet. Consume via git or path (see [Integration](integration.md)).
