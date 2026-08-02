# CLI reference

Authoritative help snapshots (generated): [cli-help.md](../reference/cli-help.md).

```text
aurum <AUDIO_FILE> [OPTIONS]
aurum models
aurum models recommend --profile balance
aurum transcribe <AUDIO_FILE> [OPTIONS]
aurum batch <INPUT> --output-dir <DIR> [OPTIONS]
aurum cleanup [TEXT_FILE] [OPTIONS]     # alias: aurum flow
aurum tts "Hello" --output-file out.wav
aurum tts models
aurum tts voices
aurum doctor
aurum support-bundle
aurum completions zsh
aurum man
aurum --help
aurum --version
```

## Transcribe options

| Flag | Default | Description |
|------|---------|-------------|
| `--provider local\|openrouter\|openai\|xai` | `local` | ASR backend (`grok` config alias → `xai`) |
| `--model <NAME>` | `base` (local) | Local ggml name or reviewed remote model id (wins over `--profile`) |
| `--profile speed\|balance\|quality` | off | Intent mapping (JOE-1723); does not change the default when omitted |
| `--language <CODE>` | `auto` | Language hint or auto-detect |
| `-o, --output txt\|srt\|json` | `txt` | Output format |
| `--output-file <PATH>` | stdout | Write to file |
| `--timestamps` | off | Request segments (implied by `srt`) |
| `--allow-unreliable-timestamps` | off | Force SRT when the route does not guarantee timestamps |
| `--cleanup <style>` | config / `raw` | `raw` \| `clean` \| `bullets` \| `professional` \| `summary` |
| `--cleanup-provider <p>` | `rules` | `rules` \| `openrouter` |
| `--cleanup-model <id>` | OpenRouter default | LLM cleanup model |
| `--cleanup-segments <p>` | `auto` | `auto` \| `keep` \| `clear` \| `per-segment` |
| `-v, --verbose` | off | Diagnostics on stderr |

## Batch options (`aurum batch`)

| Flag | Description |
|------|-------------|
| `INPUT` | File or directory of audio |
| `-O, --output-dir` | Transcripts + `aurum-batch-manifest.json` |
| `--recursive` | Walk subdirectories |
| `--resume` / `--retry-failed` | Resume / retry failed items |
| `--dry-run` | Manifest only |
| `--profile` / `--model` | Same semantics as transcribe |
| `--json` | Machine-readable summary |

## Support bundle

```bash
aurum support-bundle -O aurum-support.json
aurum support-bundle --stdout
```

Redacted offline diagnostics only (no audio, transcripts, or API keys).

## TTS options (`aurum tts`)

| Flag | Default | Description |
|------|---------|-------------|
| `TEXT` / `-` / `--input-file` | — | Exactly one UTF-8 text source |
| `--provider` | `local` | `local` \| `openrouter` \| `openai` \| `elevenlabs` \| `xai` |
| `--model` | local `kitten-nano-int8`; remote uses provider default | Local pack id or reviewed remote model id |
| `--voice` | local `Luna`; remote uses provider default | Local alias or remote voice id (no cross-provider remap) |
| `--language` | `en` | Language hint |
| `-o, --output` | `wav` | Only `wav` |
| `-O, --output-file` | required | Destination WAV path |
| `--force` | off | Overwrite existing non-empty file |
| `--speaking-rate` | `1.0` | Clamped `0.5..=2.0` |
| `--cleanup` | off | Rules-only `raw` \| `clean` |
| `--timeout` | config / `120000` | Milliseconds |
| `--local-only` | off | No download if pack missing (local only) |
| `--emit-json` | off | Honesty JSON on stdout |
| `-v` | off | Diagnostics |

When only `--provider` is set for remote TTS, model/voice default to that
provider’s reviewed defaults (not local Kitten/`Luna`). See [TTS guide](../guide/tts.md).

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
aurum interview.mp3 --provider openrouter --model openai/whisper-large-v3
aurum interview.mp3 --provider openai --model whisper-1
echo "um hello" | aurum cleanup -s clean
aurum cleanup notes.txt --style bullets -o json
aurum tts "Hello from aurum" --output-file /tmp/a.wav --emit-json
aurum tts --input-file prompt.txt -O /tmp/a.wav --voice Luna
aurum tts "Hello" --provider openai --model tts-1 --voice alloy -O /tmp/oai.wav
aurum tts models
aurum tts voices
```
