# Library overview

Aurum is split so the CLI, Rust embeds, and native embeds share one engine.

| Crate | Role |
|-------|------|
| **`aurum-core`** | Engine: STT providers, PCM, models, cleanup, output, TTS |
| **`aurum-stt`** | CLI package (`cargo install aurum-stt` → binary `aurum`) |
| **`aurum-ffi`** | C ABI façade for native hosts ([FFI guide](ffi.md)) |

## aurum-core

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
| TTS | `LocalTtsProvider`, catalogue, WAV write (feature `tts`, default on) |

```toml
aurum-core = "0.0.2"
# or git tag = "v0.0.2"
```

See [Integration](integration.md).

## Stability

!!! warning "Experimental Rust API"
    Until `aurum-core` `0.1.0`, expect breaking changes. Pin a git **tag** or **rev**.

The **C ABI** (`AURUM_ABI_VERSION`) is versioned separately and is intended to stay
narrow; see [Native embeds](ffi.md).

## crates.io

| Package | Install / depend |
|---------|------------------|
| `aurum-core` | `aurum-core = "0.0.2"` (default features include TTS; use `default-features = false` for STT-only) |
| `aurum-stt` | `cargo install aurum-stt` → runs `aurum` |
| `aurum-ffi` | `aurum-ffi = "0.0.2"` or build from source (`libaurum_ffi` + `aurum.h`) |
