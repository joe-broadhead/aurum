# Remote provider audio formats

**JOE-1937.** Remote TTS responses are untrusted parser input. Aurum never
returns provider MP3/WAV bytes as the library’s primary synthesis result.
Every remote path runs through a single bounded normalization pipeline that
produces the same contract as local TTS: **mono `i16` PCM + accurate sample
rate + duration from PCM length**.

## Pipeline

```text
provider bytes  →  BoundedAudioBody (hard encoded cap)
                →  EncodedAudioFormat (from capability / request context)
                →  normalize_remote_audio(...)
                →  NormalizedAudio { pcm_i16_mono, sample_rate_hz, duration_ms }
                →  SynthesisResult { backend_kind: Remote, ... }
```

MIME / `Content-Type` alone is **not** trusted. The call site must declare the
wire format from provider capability or request context. A mismatch fails
closed as `InvalidProviderPayload` (no broad guessing).

## Accepted wire formats

| Format | Path | Notes |
|--------|------|--------|
| Raw PCM s16le | In-process | Requires explicit `sample_rate_hz` and `channels` |
| WAV (16-bit PCM) | In-process (`hound`) | RIFF/WAVE magic required; float WAV rejected |
| MP3 | Supervised FFmpeg → mono WAV → in-process parse | Same lifecycle discipline as STT decode |

Later codecs (μ-law, A-law, Opus) are out of scope until listed here.

## Limits

Defaults (overridable via `RemoteAudioLimits`):

| Bound | Default |
|-------|---------|
| Max encoded body | 16 MiB |
| Max mono PCM samples | 48 000 × 600 (~10 min @ 48 kHz) |
| Max duration | 600 s |
| Allowed sample rates | 8 / 11.025 / 12 / 16 / 22.05 / 24 / 32 / 44.1 / 48 kHz |

Encoded and decoded caps are **independent** so a small compressed body cannot
inflate into unbounded PCM.

## Channel policy

| Policy | Behaviour |
|--------|-----------|
| `DownmixStereo` (default) | Mono passthrough; stereo averaged L+R; >2 channels rejected |
| `MonoOnly` | Only channel count 1 accepted |

Channels are never silently relabelled (e.g. stereo frames treated as mono).

## Cancellation and deadlines

`OpContext` is checked before and after decode. FFmpeg MP3 decode races cancel
and wall-clock remaining deadline; children are killed and reaped on cancel,
timeout, or failure (same supervision model as STT FFmpeg).

## Honesty / JSON

- In-memory: `tts::BackendKind::{Local, Remote}`
- `TtsMetaDto.backend_kind`: `"local"` \| `"remote"` (string; **schema_version remains 1**)
- CLI `--emit-json` uses `result.backend_kind.as_str()` (no longer hardcoded)

PCM is never included in honesty JSON (JOE-1614).

## Security

- No audio body, PCM preview, or synthesis text in error strings or support bundles
- Temp files for FFmpeg use private prefixes and are removed on every path
- Protocol whitelist for FFmpeg stays local-file oriented
