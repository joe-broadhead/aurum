# Explicit remote STT

Only when the user asks for OpenRouter / cloud STT:

```bash
export OPENROUTER_API_KEY=...   # do not echo into chat
aurum file.wav --provider openrouter
aurum file.wav --provider openrouter --openrouter-stt-mode transcriptions
```

- LLM-assisted paths may have **unreliable timestamps** — do not treat as dedicated ASR.
- Prefer `-o txt` or `-o json` unless `--allow-unreliable-timestamps` or a dedicated ASR mode is used.
- Never log the API key.
