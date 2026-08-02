# FFI snapshot

- Header: `crates/aurum-ffi/include/aurum.h`
- ABI version: 2
- Surfaces: engine create/destroy, preload, PCM STT, rules cleanup, job API, **local TTS jobs**, capabilities, doctor
- **Not in FFI:** remote providers (OpenRouter / OpenAI / ElevenLabs / xAI), microphone capture ownership, streaming loop ownership

Build:

```bash
cargo build -p aurum-ffi --release
```

CLI/library remote speech: `skills/aurum-speech/`.
