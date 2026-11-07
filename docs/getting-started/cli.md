# CLI reference

Authoritative help snapshots (generated): [cli-help.md](../reference/cli-help.md).
When this page and the binary disagree, trust `aurum --help` / the snapshot.

```text
aurum <AUDIO_FILE> [OPTIONS]
aurum models [--json]
aurum models recommend --profile balance
aurum models prepare [MODEL] [--json]
aurum transcribe <AUDIO_FILE> [OPTIONS]
aurum batch <INPUT> --output-dir <DIR> [OPTIONS]
aurum cleanup [TEXT_FILE] [OPTIONS]     # alias: aurum flow
aurum tts "Hello" --output-file out.wav
aurum tts models [--json] | voices | adapters | inspect | verify | add
aurum converse <AUDIO_FILE> --reply-text "Hello" -O out.wav
aurum converse --stdio [--preload]
aurum record -O clip.wav [--device NAME] [--seconds N] [--level-events]
aurum play clip.wav [--rate 0.5..2.0] [--device NAME]
aurum capabilities --json
aurum doctor [--json]
aurum cache status | verify | repair
aurum support-bundle
aurum completions zsh
aurum man
aurum --help
aurum --version
```

Every command accepts the global `--cache-dir <PATH>` flag. It sets
`AURUM_CACHE` for the process (and any child), so a host owns the model store
instead of rewriting `HOME`:

```bash
aurum --cache-dir ~/.cache/voice/aurum models --json
AURUM_CACHE=~/.cache/voice/aurum aurum models --json   # same store, no flag
```

## Machine-readable surface

Hosts should read these instead of parsing prose. All are offline, static, and
never load the TTS runtime:

| Command | Contents |
|---------|----------|
| `aurum capabilities --json` | version, protocol, platform, commands, caps, sidecar contract, cache root, defaults |
| `aurum models --json` | per-model id, licence, size, SHA-256, revision, cache state, enabled |
| `aurum tts models --json` | TTS packs with licence, pins, cache state, adapter |
| `aurum doctor --json` | versioned checks plus `engine`, `platform`, `models`, `audio`, `microphone` |

`aurum capabilities --json` is the superset used by `opencode-voice`:

```json
{"schema_version":1,"protocol_version":1,
 "platform":{"os":"macos","arch":"aarch64","target":"aarch64-macos"},
 "cache_root":"/Users/me/.cache/voice/aurum",
 "caps":{"stdio_sidecar":true,"long_audio":true,"max_audio_secs":7200,
         "synthesize_speaking_rate":true,"synthesize_abort":true,
         "record":true,"play":true,"cache_dir_flag":true,"cache_env":"AURUM_CACHE"},
 "sidecar":{"protocol_version":1,"caps":["transcribe","synthesize",
            "synthesize_speaking_rate","synthesize_abort","long_audio"]}}
```

## Device capture and playback

`record` and `play` use Aurum's existing `cpal` stack, so a host does not need
`ffmpeg`/avfoundation for capture or `afplay` for playback.

```bash
# Dictation clip: 16 kHz mono PCM WAV, stop after 5 s, JSONL levels on stdout
aurum record -O ~/clip.wav --seconds 5 --level-events

# Choose a device by substring; play at 1.25x
aurum record -O ~/clip.wav --device "MacBook"
aurum play ~/clip.wav --rate 1.25
```

`record` writes the same format the STT path and sidecar accept, so
`aurum record … && aurum clip.wav` works with no conversion. Without
`--seconds` it records until Ctrl-C (the WAV is still written).
`--level-events` emits `{"type":"level","t_ms":…,"rms":…,"peak":…}` about ten
times a second, then `{"type":"recorded",…}`; a host can drive a level meter
without parsing audio.

`play` accepts PCM WAV at any rate (downmixed to mono). `--rate` is a tape-speed
multiplier: it changes duration and pitch together.

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
| `--openrouter-stt-mode auto\|chat\|transcriptions` | `auto` | OpenRouter route only; unknown models fail closed in `auto` |
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
| `-s, --style` | `clean`* | Cleanup style (`*` defaults to `clean` if config is `raw`) |
| `--provider` | `rules` | Backend |
| `--model` | config | OpenRouter model |
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
