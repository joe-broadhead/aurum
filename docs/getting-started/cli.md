# CLI reference

```text
aurum <AUDIO_FILE> [OPTIONS]
aurum models
aurum transcribe <AUDIO_FILE> [OPTIONS]
aurum cleanup [TEXT_FILE] [OPTIONS]     # alias: aurum flow
aurum --help
aurum --version
```

## Transcribe options

| Flag | Default | Description |
|------|---------|-------------|
| `--provider local\|openrouter` | `local` | ASR backend |
| `--model <NAME>` | `base` (local) | Local ggml name or OpenRouter model id |
| `--language <CODE>` | `auto` | Language hint or auto-detect |
| `-o, --output txt\|srt\|json` | `txt` | Output format |
| `--output-file <PATH>` | stdout | Write to file |
| `--timestamps` | off | Request segments (implied by `srt`) |
| `--allow-unreliable-timestamps` | off | Force SRT on OpenRouter |
| `--cleanup <style>` | config / `raw` | `raw` \| `clean` \| `bullets` \| `professional` \| `summary` |
| `--cleanup-provider <p>` | `rules` | `rules` \| `openrouter` |
| `--cleanup-model <id>` | OpenRouter default | LLM cleanup model |
| `--cleanup-segments <p>` | `auto` | `auto` \| `keep` \| `clear` \| `per-segment` |
| `-v, --verbose` | off | Diagnostics on stderr |

## Cleanup-only options

| Flag | Default | Description |
|------|---------|-------------|
| `TEXT_FILE` | stdin | Input text |
| `-s, --style` / `--cleanup` | `clean`* | Cleanup style (`*` defaults to `clean` if config is `raw`) |
| `--provider` / `--cleanup-provider` | `rules` | Backend |
| `--model` / `--cleanup-model` | config | OpenRouter model |
| `-o txt\|json` | `txt` | Output |
| `--output-file` | stdout | Write path |

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 2 | User error (missing file, bad model, missing API key) |
| 3 | Environment error (ffmpeg missing, I/O) |
| 4 | Provider error (download, network, inference) |
| 1 | Internal |

## Examples

```bash
aurum models
aurum interview.mp3 --model small-q5_1 --language en -o json
aurum interview.mp3 --cleanup clean --cleanup-segments keep
aurum interview.mp3 --provider openrouter --model openai/gpt-audio-mini
echo "um hello" | aurum cleanup -s clean
aurum cleanup notes.txt --style bullets -o json
```
