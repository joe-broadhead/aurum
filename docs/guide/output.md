# Output formats

| Format | Description |
|--------|-------------|
| `txt` | Plain transcript text |
| `srt` | SubRip cues (local ASR timestamps) |
| `json` | Structured result |

## JSON shape

```json
{
  "text": "…",
  "language": "en",
  "model": "tiny-q5_1",
  "provider": "local",
  "duration_secs": 5.29,
  "backend_kind": "asr",
  "timestamps_reliable": true,
  "segments": [
    { "start": 0.0, "end": 3.9, "text": "…" }
  ]
}
```

For OpenRouter, `backend_kind` is `llm_assisted` and `timestamps_reliable` is
always `false`.
