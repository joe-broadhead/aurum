# Output formats

| Format | Description |
|--------|-------------|
| `txt` | Plain transcript text (default) |
| `srt` | SubRip cues from ASR segments |
| `json` | Structured result |

```bash
aurum talk.m4a -o txt
aurum talk.m4a -o srt --output-file talk.srt
aurum talk.m4a -o json
```

## JSON shape

```json
{
  "text": "Hello world.",
  "language": "en",
  "model": "tiny-q5_1",
  "provider": "local",
  "duration_secs": 5.29,
  "backend_kind": "asr",
  "timestamps_reliable": true,
  "cleanup_style": "raw",
  "segments": [
    { "start": 0.0, "end": 3.9, "text": "Hello world." }
  ]
}
```

After `--cleanup clean` (rules):

| Field | Meaning |
|-------|---------|
| `cleanup_style` | e.g. `"clean"` |
| `cleanup_provider` | `"rules"` or `"openrouter"` |
| `original_text` | Pre-cleanup ASR string |

### Honesty fields

| Field | Local ASR | OpenRouter ASR |
|-------|-----------|----------------|
| `backend_kind` | `asr` | `llm_assisted` |
| `timestamps_reliable` | `true` | `false` |

OpenRouter SRT is refused unless `--allow-unreliable-timestamps` is set.
