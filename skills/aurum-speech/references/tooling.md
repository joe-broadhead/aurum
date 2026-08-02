# Aurum tooling around STT/TTS

Agents should use these commands instead of ad-hoc shell work.

## Install / version

```bash
# Preferred: verified GitHub Release binary
./scripts/install.sh --from-release
# Or pin:
AURUM_VERSION=v0.0.20 ./scripts/install.sh --from-release

cargo install aurum-stt --locked
aurum --version
```

From source (Rust 1.89+, cmake, ffmpeg):

```bash
./scripts/install.sh --from-source
```

## Doctor and capabilities

```bash
aurum doctor
aurum doctor --json
```

Read-only system/config/cache/capability diagnostics. Use first on “it doesn’t work.”

## Models and cache (STT)

```bash
aurum models
aurum models recommend --profile balance
aurum cache status
aurum cache verify
```

- Profiles: `speed` | `balance` | `quality` (opt-in).
- Do not select experimental STT models (e.g. `large-v3-q5_0`) via profiles.

## TTS catalogue

```bash
aurum tts models
aurum tts voices
aurum tts adapters
aurum tts inspect <pack>
aurum tts verify <pack>
```

Local packs are adapter-bound (not “load any ONNX”).

## Cleanup-only (no audio)

```bash
echo "um, so, hello" | aurum cleanup -s clean
aurum cleanup notes.txt --style professional -o json
# alias:
aurum flow -s clean < notes.txt
```

## Batch (multi-file STT)

```bash
aurum batch INPUT -O OUT --dry-run
aurum batch INPUT -O OUT [--model …] [--profile …] [--resume] [--retry-failed]
```

Manifest: `OUT/aurum-batch-manifest.json` (schema version 1).  
See `skills/aurum-batch/`.

## Support (privacy-safe)

```bash
aurum support-bundle -O aurum-support.json
aurum support-bundle --stdout
```

Never attach raw audio, `.env`, or secrets. See `skills/aurum-support/`.

## Completions / man

```bash
aurum completions zsh
aurum man
```

## Config (optional)

Platform path via `directories` (app `aurum`), e.g. macOS:

`~/Library/Application Support/aurum/config.toml`

```toml
[stt]
provider = "local"
model = "base"

[tts]
provider = "local"
model = "kitten-nano-int8"
voice = "Luna"
```

Secrets: prefer env vars (`OPENROUTER_API_KEY`, `OPENAI_API_KEY`,
`ELEVENLABS_API_KEY`, `XAI_API_KEY`). Config file keys are redacted in doctor/support.

Full schema: `docs/guide/configuration.md`.

## Troubleshooting quick map

| Symptom | Action |
|---------|--------|
| ffmpeg missing | install ffmpeg; `aurum doctor` |
| first run slow | expected download; try `--model tiny-q5_1` |
| OpenRouter privacy/guardrail | user fixes OpenRouter privacy settings |
| SRT refused | use txt/json or dedicated ASR; or `--allow-unreliable-timestamps` |
| long remote lecture | auto chunk~210s (JOE-2212); or local whisper offline; override `AURUM_REMOTE_STT_CHUNK_SECS` |
| TTS overwrite refused | add `--force` |
| TTS pack missing offline | drop `--local-only` once online, or pre-cache |
| Metal exit abort (lib) | `clear_context_cache` / `aurum_shutdown` before process exit |

Details: `docs/operations/troubleshooting.md`.
