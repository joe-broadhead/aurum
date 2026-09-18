# Live speech (`aurum converse`)

Two supported products, same engine:

| Mode | Who owns mic / brain |
|------|----------------------|
| **`--stdio` sidecar** | Host app (Jelly, OpenCode, Pi RPC) |
| **`--mic` CLI loop** | Aurum CLI (`--llm-provider` optional chat) |

Library: `aurum_core::live::LiveSession` (`tts` feature) — no devices, no LLM.

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

## Standalone CLI conversation

`converse --mic` and `--llm-provider` are **supported CLI modes** for a
self-contained voice loop (Aurum opens the default mic/speakers and may call
chat). Use this when there is no session host.

Session apps (Jelly, OpenCode, Pi with tools) must still use `--stdio` so the
harness stays the brain. Do not pass `--llm-provider` on a sidecar.

```bash
# Local STT/TTS + OpenAI chat (headphones; half-duplex; Ctrl+C to stop)
aurum converse --mic --model tiny-q5_1 --llm-provider openai

# All OpenAI speech + chat
aurum converse --mic \
  --provider openai --model gpt-4o-mini-transcribe \
  --tts-provider openai --tts-model tts-1 --voice alloy \
  --llm-provider openai --llm-model gpt-4o-mini
```

Constraints (supported, not “try at your own risk”):

- Half-duplex: mic is ignored while the agent speaks; no AEC / barge-in
- Headphones recommended
- RMS endpoint (~0.45 s silence); not a neural VAD
- Chat is non-session: no tools/MCPs; streaming sentences + TTS overlap
- `converse --stdio` without `--mic` never opens an audio device

Pi glue (CLI mic + Pi brain): `python3 scripts/aurum-pi-voice.py -- --mic --model tiny-q5_1`.

## Non-goals

AEC / barge-in · streaming ASR/TTS · FFI live jobs · LLM inside `aurum-core` ·
microphone ownership in the library.
