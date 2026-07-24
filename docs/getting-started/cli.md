# CLI reference

```text
aurum <AUDIO_FILE> [OPTIONS]
aurum models
aurum transcribe <AUDIO_FILE> [OPTIONS]
aurum cleanup [TEXT_FILE] [--style …]     # also: aurum flow
aurum --help
aurum --version
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--provider local\|openrouter` | `local` | Backend |
| `--model <NAME>` | `base` (local) / OpenRouter default | Model id |
| `--language <CODE>` | `auto` | Language or auto-detect |
| `-o, --output txt\|srt\|json` | `txt` | Format |
| `--output-file <PATH>` | stdout | Write to file |
| `--timestamps` | off | Request segments (implied by `srt`) |
| `--allow-unreliable-timestamps` | off | Force SRT on OpenRouter |
| `--cleanup <style>` | config / `raw` | Post-ASR flow style |
| `--cleanup-provider <rules\|openrouter>` | `rules` | Cleanup backend |
| `--cleanup-model <id>` | openrouter default | LLM cleanup model |
| `--cleanup-segments <auto\|keep\|clear\|per-segment>` | `auto` | Segment policy after cleanup |
| `-v, --verbose` | off | Diagnostics on stderr |

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
aurum interview.mp3 --provider openrouter --model google/gemini-2.5-flash
aurum interview.mp3 --cleanup clean
echo "um hello" | aurum cleanup -s clean
aurum cleanup notes.txt --style bullets -o json
```
