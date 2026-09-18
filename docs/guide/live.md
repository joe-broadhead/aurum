# Live conversation (turn-based)

Aurum can drive a **half-duplex conversation turn**: audio file (or host PCM) →
STT → host/agent text → TTS WAV. It does **not** own the microphone, does **not**
stream, and does **not** run an LLM.

Library: `aurum_core::live::LiveSession` (requires the `tts` feature).  
CLI: `aurum converse` (file replay, `--mic`, or `--stdio` sidecar).

## Defaults

Local STT + local TTS. Remote providers need an explicit `--provider` /
`--tts-provider` **and** the matching key. Key presence never selects a provider.

Inbound PCM is **ignored while the agent is speaking**.

## CLI

```bash
# Live mic, snappy OpenAI (headphones; Ctrl+C to stop)
cargo run -p aurum-stt -- converse --mic \
  --provider openai --model gpt-4o-mini-transcribe \
  --tts-provider openai --tts-model tts-1 --voice alloy \
  --llm-provider openai --llm-model gpt-4o-mini

aurum converse tests/fixtures/sample.wav --reply-text "Hello" -O /tmp/out.wav --force

# LLM agent turn (explicit provider; never inferred from keys)
aurum converse talk.wav --llm-provider openai -O /tmp/out.wav --force --emit-json

# Mixed cloud speech + LLM
aurum converse talk.wav --provider openai --tts-provider elevenlabs \
  --voice 21m00Tcm4TlvDq8ikWAM --llm-provider openrouter \
  -O /tmp/out.wav --force --emit-json
```

## Harnesses (Pi, OpenCode, …)

`--stdio` makes Aurum a **speech sidecar**: no in-process LLM. JSONL on stdio
(LF lines, optional CR). Logs stay on **stderr**. No PCM, no secrets.

Stdout events: `ready`, `user_final`, `listening`, `error`, `shutdown`.  
Stdin commands: `speak`, `end_turn`, `abort`, `shutdown`.

```bash
# Sidecar only (you wire the brain)
cargo run -p aurum-stt -- converse --mic --stdio --model tiny-q5_1

# Pi RPC glue (tools + MCPs in Pi)
python3 scripts/aurum-pi-voice.py -- --mic --model tiny-q5_1
```

OpenCode: use the same JSONL against `opencode run --format json` / ACP / serve.
Do not pass `--llm-provider` with `--stdio`.

`--reply-text` / `--reply-file` / `--llm-provider` / `--stdio` are mutually exclusive.
`--local-only` rejects remote STT/TTS **and** `--llm-provider`.
`--emit-json` prints STT + TTS honesty metadata (no PCM); LLM provider/model/text
when used.

When `--tts-provider elevenlabs` is set and `--tts-model` is omitted **and**
config still has a local model id, converse prefers `eleven_flash_v2_5`. This
does not change `aurum tts` defaults. ElevenLabs still requires a real
`voice_id` (never remapped from Luna).

## Library

```rust,no_run
use aurum_core::live::{LiveSession, LiveSessionConfig};
use aurum_core::AurumEngine;

# async fn demo() -> aurum_core::Result<()> {
let engine = AurumEngine::load()?;
let mut live = LiveSession::new(engine, LiveSessionConfig::default())?;
live.push_pcm(&/* 16 kHz mono f32 */ vec![0.0; 1600])?;
live.end_user_turn()?;
let _stt = live.commit_user_turn().await?;
let _tts = live.speak_text("Hello from aurum").await?;
live.end_agent_turn()?;
live.shutdown();
# Ok(())
# }
```

STT and TTS providers are whatever the engine config already has (`[stt]` /
`[tts]`). `LiveSessionConfig` is endpoint/speak-ahead policy only.

## Non-goals

AEC / barge-in · streaming ASR/TTS · FFI live jobs · LLM inside `aurum-core`
(CLI `--llm-provider` only). `aurum converse --mic` is experimental CLI device I/O.
