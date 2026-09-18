# Speech sidecar (live / session hosts)

Aurum is **ears and mouth**. The host owns the microphone, speakers, and the
agent brain (OpenCode, Pi, Jelly, …). This is the supported embed contract.

Library: `aurum_core::live::LiveSession` (`tts` feature).  
CLI daemon: `aurum converse --stdio`.

Defaults remain **local** STT + local TTS. Remote needs explicit `--provider` /
`--tts-provider` **and** the matching key. Keys never select a provider.
`streaming_advertised` is **false**.

## Sidecar (`--stdio`)

Long-lived process. JSONL on stdio (LF; optional CR). **stderr** = logs.
No PCM and no secrets on the wire.

```bash
aurum converse --stdio --local-only --model tiny-q5_1
```

Do **not** pass `--mic` or `--llm-provider`. Desktop apps keep TCC on the app
bundle; they write private WAVs and call `transcribe` / `synthesize`.

### `ready`

```json
{"v":1,"type":"ready","stt_provider":"local","tts_provider":"local","caps":["transcribe","synthesize","speak"]}
```

`speak` is only honored when the process was started with `--mic` (local
playback). Hosts that own speakers use `synthesize`.

### Commands (stdin)

| type | fields | effect |
|------|--------|--------|
| `transcribe` | `path` (absolute file) | STT; emits `user_final` |
| `synthesize` | `text`, `path` (absolute WAV out) | TTS write; **does not play**; emits `synthesized` |
| `speak` | `text` | Play on default speaker; **requires `--mic`** |
| `end_turn` | | Drain playback if any; back to listening |
| `abort` | | Cancel in-flight STT/TTS; reset turn |
| `shutdown` | | Exit after `shutdown` event |

Paths must be absolute (no NUL/newline). Relative paths → `error` `category=user`.

### Events (stdout)

| type | notes |
|------|--------|
| `user_final` | `text`, `provider`, `model` |
| `synthesized` | `path`, `duration_ms`, `provider`, `model`, `voice`, `sample_rate_hz` |
| `listening` | after `end_turn` / `abort` |
| `error` | `category` (`user` \| `environment` \| `provider` \| `internal`) + `message` |
| `shutdown` | process exiting |

Broken stdout (EPIPE) exits the sidecar with a non-zero status. Overlapping
`transcribe`/`synthesize` while one is running returns `error` `category=provider`
(`busy`). `abort` sets the session cancel flag immediately.

`--stdio` is incompatible with `--llm-provider`, `--reply-text`, `--reply-file`,
and `--emit-json`.

## Library hosts

Prefer `AurumEngine` + `LiveSession` in-process when the host is Rust.
`LiveSession` is **not** in `prelude`. The host resamples to 16 kHz mono f32,
calls `commit_user_turn`, and uses `engine.synthesize` (or `speak_text`) for TTS.
Call `shutdown` then `clear_context_cache()` before process exit (Metal).

## CLI-only (not the embed contract)

`aurum converse --mic` and `--llm-provider` are **CLI demos**: they open local
devices and/or call chat completions inside Aurum. Session apps must not use
them. Glue example: `scripts/aurum-pi-voice.py` (unsupported).

```bash
# Demo only — not for Jelly/OpenCode
aurum converse --mic --model tiny-q5_1 --llm-provider openai
```

The `aurum` CLI links `cpal`. `converse --stdio` without `--mic` never opens an
audio device.

## Non-goals

AEC / barge-in · streaming ASR/TTS · FFI live jobs · LLM inside `aurum-core` ·
microphone ownership in the library.
