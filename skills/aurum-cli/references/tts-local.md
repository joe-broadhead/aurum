# Local TTS

```bash
aurum tts "Hello from aurum" --output-file /tmp/a.wav --force
aurum tts models
aurum tts voices
aurum tts --input-file notes.txt -O /tmp/out.wav --force
```

- Local ONNX only — **no remote TTS**.
- Default model/voice come from config (`kitten-nano-int8` / Luna).
- Use `--local-only` to refuse downloads.
