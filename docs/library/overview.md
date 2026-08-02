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
| ASR | `TranscriptionProvider`, local whisper, OpenRouter / OpenAI / xAI remotes |
| Engine | `AurumEngine`, `ValidatedConfig`, provider registry / factories |
| Audio | `load_audio`, `AudioInput::from_pcm`, safety limits |
| PCM / mic hosts | `PcmBuffer`, `transcribe_pcm`, `preload`, `local_only` |
| Partials (host-driven) | `PartialWindowPolicy`, `PartialClock` |
| Cancel | `CancelFlag` / `OpContext` in options |
| Models | catalogue, download, pins, progress callbacks |
| Cleanup | `RulesCleanup`, `OpenRouterCleanup`, `apply_cleanup*` |
| Output | `format_result` — txt / srt / json |
| Post-ASR | special-token strip, timestamp clamp |
| TTS | local Kitten/Kokoro + remote OpenRouter / OpenAI / ElevenLabs / xAI (feature `tts`, default on) |

```toml
aurum-core = "0.0.22"
# or git tag = "v0.0.22"
```

See [Integration](integration.md) and [AurumEngine](engine.md).

## Stability

!!! warning "Provisional Rust API (0.0.x)"
    On the continuous **0.0.x** line, expect breaking changes between patches when
    necessary. Pin a crates.io version, git **tag**, or **rev**. A stable major
    (`1.0.0`) is not planned for the current programme; next assurance cut is **0.0.22**.

The **C ABI** (`AURUM_ABI_VERSION`) is versioned separately and is intended to stay
narrow; remote providers are **not** exposed on the FFI. See [Native embeds](ffi.md).

## crates.io

| Package | Install / depend |
|---------|------------------|
| `aurum-core` | `aurum-core = "0.0.22"` (default features include TTS; use `default-features = false` for STT-only) |
| `aurum-stt` | `cargo install aurum-stt` → runs `aurum` |
| `aurum-ffi` | `aurum-ffi = "0.0.22"` or build from source (`libaurum_ffi` + `aurum.h`) |
