# Aurum

<div align="center">

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.89%2B-orange.svg?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Docs](https://img.shields.io/badge/docs-mkdocs%20material-blue.svg?logo=materialformkdocs&logoColor=white)](https://joe-broadhead.github.io/aurum/)
[![CI](https://github.com/joe-broadhead/aurum/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/joe-broadhead/aurum/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/joe-broadhead/aurum?include_prereleases&logo=github)](https://github.com/joe-broadhead/aurum/releases)
[![crates.io aurum-core](https://img.shields.io/crates/v/aurum-core.svg?logo=rust)](https://crates.io/crates/aurum-core)
[![crates.io aurum-stt](https://img.shields.io/crates/v/aurum-stt.svg?logo=rust)](https://crates.io/crates/aurum-stt)
[![docs.rs aurum-core](https://docs.rs/aurum-core)](https://docs.rs/aurum-core)

</div>

```
    _                              
   / \  _   _ _ __ _   _ _ __ ___  
  / _ \| | | | '__| | | | '_ ` _ \ 
 / ___ \ |_| | |  | |_| | | | | | |
/_/   \_\__,_|_|   \__,_|_| |_| |_|
                                   
   Speech both ways.
   On-device by default.
```

<div align="center">

**Speech both ways. On-device by default.**
No API key required · local whisper + KittenTTS · optional remote providers · reusable **`aurum-core`**

[Docs](https://joe-broadhead.github.io/aurum/) ·
[Quickstart](docs/getting-started/quickstart.md) ·
[Providers](docs/guide/providers.md) ·
[Library](docs/library/integration.md) ·
[Security](SECURITY.md)

</div>

---

## What it does

Aurum is an on-device **speech CLI** and **Rust library**:

1. **STT** — audio file → text with **whisper.cpp** (Metal on macOS)
2. Optionally **clean** the text (fillers, bullets, professional, summary)
3. Emit **`txt`**, **`srt`**, or **`json`**
4. **TTS** — `aurum tts "…"` → mono **WAV** on-device (KittenTTS / Kokoro ONNX; no API key)

Remote STT/TTS (`openrouter`, `openai`, `elevenlabs`, `xai`) is **optional and
explicit** — never the default. Local whisper + Kitten remain zero-key defaults.
See [provider matrix](docs/guide/provider-matrix.md) and
[qualification](docs/operations/provider-qualification.md).

> **v0.0.22** — product outcomes & SDK coherence (JOE-2215): STT/TTS evidence programmes, long-form fidelity, batch v2, SDK contracts, observability, provider evidence gate, product contracts, Tier A native SDK. Library API remains provisional on continuous **0.0.x** (not 1.0). Published as `v0.0.22`.

## Highlights

| | |
|--|--|
| **Local by default** | whisper.cpp STT + ONNX TTS — no API key |
| **Fast first run** | Quantized STT e.g. `tiny-q5_1` (~32 MB); TTS pack ~26 MB |
| **Batch** | `aurum batch` with resume manifests |
| **Embeddable** | PCM STT API · `aurum-ffi` (local STT + TTS jobs) · library TTS |
| **Cleanup / flow** | On-device rules or OpenRouter LLM (`aurum cleanup`) |
| **Local TTS** | `aurum tts` · Kitten + opt-in Kokoro · pinned SHA-256 packs |
| **Honest remote** | Opt-in OpenRouter / OpenAI / ElevenLabs / xAI; SRT fail-closed when timestamps unreliable |
| **Scriptable** | Exit codes · JSON · completions · man · support bundles |

## 30-second install

```bash
# Verified GitHub Release binary (recommended)
curl -fsSL https://raw.githubusercontent.com/joe-broadhead/aurum/master/scripts/install.sh \
  | bash -s -- --from-release

# Or from source (Rust 1.89+, cmake, ffmpeg)
git clone https://github.com/joe-broadhead/aurum.git && cd aurum
./scripts/install.sh --from-source

aurum models
aurum models recommend --profile balance
aurum meeting.m4a --model tiny-q5_1
aurum batch ./lectures -O ./out --profile speed
aurum meeting.m4a --cleanup clean
echo "um, hello there" | aurum cleanup -s clean
aurum tts "Hello from aurum" -O /tmp/hello.wav
aurum support-bundle -O /tmp/aurum-support.json
```

Agent skills for coding agents: [`skills/`](skills/) — start with
[`skills/aurum-speech/`](skills/aurum-speech/) for all STT/TTS work.

## Workspace

| Crate | Role |
|-------|------|
| [`aurum-core`](crates/aurum-core) | Reusable library |
| [`aurum-stt`](crates/aurum) | CLI binary (`aurum`) |
| [`aurum-ffi`](crates/aurum-ffi) | C ABI for native embeds ([`aurum.h`](crates/aurum-ffi/include/aurum.h)) |

```toml
# Prefer crates.io when published, or pin a git tag
aurum-core = "0.0.22"
# aurum-core = { git = "https://github.com/joe-broadhead/aurum", package = "aurum-core", tag = "v0.0.22" }
```

Full guide: [Library integration](https://joe-broadhead.github.io/aurum/library/integration/).

## CLI cheatsheet

```bash
aurum <FILE> [--model NAME] [--profile speed|balance|quality] [-o txt|srt|json] [--cleanup STYLE]
aurum models
aurum models recommend --profile balance
aurum batch <INPUT> -O <DIR> [--resume] [--retry-failed]
aurum cleanup [TEXT_FILE] --style clean   # alias: aurum flow
aurum tts "Hello" -O out.wav [--voice Luna]
aurum tts models && aurum tts voices
aurum support-bundle -O support.json
aurum completions zsh
# Opt-in remote examples:
aurum <FILE> --provider openrouter --model openai/whisper-large-v3
aurum tts "Hello" --provider openai --model tts-1 --voice alloy -O /tmp/oai.wav
```

| Flag | Default | Notes |
|------|---------|--------|
| `--provider` | `local` | STT: `local` \| `openrouter` \| `openai` \| `xai` · TTS also: `elevenlabs` |
| `--model` | `base` (local STT) | ggml name, or a reviewed remote model id |
| `--profile` | off | `speed` \| `balance` \| `quality` (opt-in; does not change default alone) |
| `-o` / `--output` | `txt` | `txt` \| `srt` \| `json` |
| `--cleanup` | `raw` (off) | `clean` \| `bullets` \| `professional` \| `summary` |
| `--cleanup-provider` | `rules` | `rules` (on-device) \| `openrouter` |

## Documentation

| Topic | Link |
|-------|------|
| Installation | [docs/getting-started/installation.md](docs/getting-started/installation.md) |
| Quickstart | [docs/getting-started/quickstart.md](docs/getting-started/quickstart.md) |
| CLI reference | [docs/getting-started/cli.md](docs/getting-started/cli.md) |
| Providers | [docs/guide/providers.md](docs/guide/providers.md) |
| Provider matrix | [docs/guide/provider-matrix.md](docs/guide/provider-matrix.md) |
| Models | [docs/guide/models.md](docs/guide/models.md) |
| Cleanup | [docs/guide/cleanup.md](docs/guide/cleanup.md) |
| TTS | [docs/guide/tts.md](docs/guide/tts.md) |
| Configuration | [docs/guide/configuration.md](docs/guide/configuration.md) |
| Native embeds (FFI) | [docs/library/ffi.md](docs/library/ffi.md) |
| Architecture | [docs/development/architecture.md](docs/development/architecture.md) |
| Release gate | [docs/operations/release-gate.md](docs/operations/release-gate.md) |
| Security | [SECURITY.md](SECURITY.md) |

Site: **https://joe-broadhead.github.io/aurum/**

## Docs site (local)

```bash
python3 -m venv .venv && .venv/bin/pip install -r docs/requirements.txt
.venv/bin/mkdocs serve
```

## Development

```bash
make ci # fmt + clippy + test + version-check
cargo test --workspace --locked
cargo test -p aurum-core --test local_integration -- --ignored
```

## Release

Maintainer-only. Prepare → merge `release/x.y.z` → tag → multi-platform binaries.
Details: [docs/development/release.md](docs/development/release.md).

## Versioning

Aurum ships on a continuous **0.0.x** line. Production-assurance work formerly
aimed at “1.0” is retargeted to continuous **0.0.x** (tip **v0.0.22**). Pin tags or crates.io versions;
do not assume a stable major version yet.

## Non-goals (0.0.x)

Built-in microphone capture · speaker diarization · stable library major API ·
remote execution on the C ABI · multi-tenant isolation in one process.

## License

[MIT](LICENSE)
