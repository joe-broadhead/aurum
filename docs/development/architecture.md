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
  model/        STT catalogue, download, lock, integrity
  providers/    local whisper.cpp · openrouter
  postprocess/  ASR markers, clamp, NaN guard
  cleanup/      rules | openrouter LLM
  tts/          adapters · packs · catalogue · KittenTTS · BYOM · wav
  output/       txt · srt · json (STT)
  config/
  error/        user | environment | provider
```

## Pipelines

### STT

```mermaid
flowchart LR
  A[File or PCM] --> B[AudioInput]
  B --> C[TranscriptionProvider]
  C --> D[postprocess]
  D --> E[optional TextCleanup]
  E --> F[format_result]
```

### TTS

```mermaid
flowchart LR
  T[UTF-8 text] --> V[validate + optional rules clean]
  V --> G[G2P misaki-rs MIT]
  G --> O[ONNX KittenTTS]
  O --> P[peak guard PCM]
  P --> W[atomic mono WAV]
```

STT and TTS share error taxonomy, cache root, cancel flags, and config loading — but **modules stay separate** (`providers/` vs `tts/`). The C ABI (**v2**) exposes local STT, rules cleanup, and **local TTS jobs** ([FFI guide](../library/ffi.md)); remote OpenRouter and microphone ownership remain out of the FFI surface.

## Design rules

1. **CLI owns UX** — progress, first-run tips, OpenRouter SRT policy, TTS overwrite policy, batch manifests, support bundles  
2. **Core owns truth** — providers return normalized results + honesty fields  
3. **FFI owns a narrow embed surface** — PCM, preload, cancel, rules cleanup, local TTS jobs, doctor/capabilities ([guide](../library/ffi.md)); contracts remain provisional until 1.0  
4. **Fail closed** — bad magic, oversized audio, missing keys, offline missing models/voices, bad SHA-256  
5. **No default network** except explicit model/voice download or remote STT provider  
6. **ASR ≠ cleanup ≠ TTS** — separate stages and modules  
7. **MIT binary for default TTS** — no GPL-linked phonemizer on the default path ([TTS guide](../guide/tts.md))

## Public contracts (JOE-1575)

Validated **config**, **capability preflight**, **versioned JSON DTOs**, and a
stable **error category** map sit at the library boundary. See
[compatibility.md](compatibility.md) and [migration-0.0.3.md](migration-0.0.3.md).

## Performance & progressive STT (JOE-1574)

Hot paths keep a **bounded working set**: ring-buffer PCM, streaming decode, and
streaming output writers. Hosts drive progressive text via **`PartialSession`**
(stable/unstable revisions, supersession cancel) without Aurum owning the mic.
Reproducible smoke benches and a versioned WER/CER eval corpus live under
`aurum_core::bench` / `aurum_core::eval` and `evals/`.

## Native SDK & FFI v2 (JOE-1577)

Embedders use an explicit **engine** plus optional **jobs** so hosts never nest
Tokio. Blocking v1 exports remain; jobs are the nonblocking path. Process
shutdown is global; engine shutdown drains only that engine's jobs. See
[FFI guide](../library/ffi.md).

## TTS model platform (JOE-1576)

TTS engines are **adapters**, not bare ONNX paths. Packs carry a versioned
manifest (`aurum-tts-manifest.json`) with trust modes (`builtin` / `verified` /
`local_unverified`). Kitten remains the default shipped engine; `fake-sine-v1`
proves the contract is not Kitten-specific; Kokoro ships as opt-in catalogue
model `kokoro-82m-int8` ([ADR-001](adr-001-kokoro-tts-adapter.md), JOE-1618). Custom
catalogue entries and `aurum tts inspect|verify|add` fail closed on unknown
adapters, digest mismatch, and reserved ids.

## Process model cache & runtime (JOE-1573)

`LocalWhisperProvider` loads Whisper contexts through **singleflight** (one native
load per key under concurrency) and a **weighted residency registry**. TTS sessions
use the same singleflight pattern. A process **`ResourceGovernor`** admits model
loads, local STT/TTS jobs, remote work, blocking pool slots, CPU threads, and soft
memory reservations — overload returns typed `ProviderError::Overload` rather than
exhausting the host.

FFI uses a synchronized **lifecycle** (`Running → ShuttingDown → Stopped`). Prefer
`aurum_shutdown_ex` when the host needs a `BUSY` status; context caches clear only
when the active-op count reaches zero. Drop contexts via `clear_context_cache()`
(or successful shutdown) before process exit so Metal/ggml teardown does not assert.
