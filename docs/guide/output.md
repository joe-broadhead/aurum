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
  "cleanup_style": "raw",
  "segments": [
    { "start": 0.0, "end": 3.9, "text": "…" }
  ]
}
```

After `--cleanup clean` (rules), JSON also includes:

- `cleanup_style`: e.g. `"clean"`
- `cleanup_provider`: `"rules"` or `"openrouter"`
- `original_text`: pre-cleanup ASR string

For OpenRouter **transcription**, `backend_kind` is `llm_assisted` and
`timestamps_reliable` is always `false` (independent of cleanup).
