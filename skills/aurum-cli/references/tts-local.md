# Local TTS (pointer)

Prefer **`skills/aurum-speech/references/tts.md`** for local + remote TTS.

```bash
aurum tts "Hello from aurum" --output-file /tmp/a.wav --force
aurum tts models
aurum tts voices
aurum tts --input-file notes.txt -O /tmp/out.wav --force
```

- Local ONNX default (`kitten-nano-int8` / Luna); remote is opt-in via `--provider`.
- Use `--local-only` to refuse downloads.
