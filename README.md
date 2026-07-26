# Aurum

<div align="center">

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.89%2B-orange.svg?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Docs](https://img.shields.io/badge/docs-mkdocs%20material-blue.svg?logo=materialformkdocs&logoColor=white)](https://joe-broadhead.github.io/aurum/)
[![CI](https://github.com/joe-broadhead/aurum/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/joe-broadhead/aurum/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/joe-broadhead/aurum?include_prereleases&logo=github)](https://github.com/joe-broadhead/aurum/releases)
[![crates.io aurum-core](https://img.shields.io/crates/v/aurum-core.svg?logo=rust)](https://crates.io/crates/aurum-core)
[![crates.io aurum-stt](https://img.shields.io/crates/v/aurum-stt.svg?logo=rust)](https://crates.io/crates/aurum-stt)
[![docs.rs aurum-core](https://img.shields.io/docsrs/aurum-core?logo=docsdotrs)](https://docs.rs/aurum-core)

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
No API key required · clean providers · optional cleanup styles · reusable **`aurum-core`**

[Docs](https://joe-broadhead.github.io/aurum/) ·
[Quickstart](docs/getting-started/quickstart.md) ·
[Library](docs/library/integration.md) ·
[Cleanup](docs/guide/cleanup.md) ·
[Security](SECURITY.md)

</div>

---

## What it does

Aurum is an on-device **speech CLI** and **Rust library**:

1. **STT** — audio file → text with **whisper.cpp** (Metal on macOS)
2. Optionally **clean** the text (fillers, bullets, professional, summary)
3. Emit **`txt`**, **`srt`**, or **`json`**
4. **TTS** — `aurum tts "…"` → mono **WAV** on-device (KittenTTS ONNX; no API key)

OpenRouter is an **optional** remote path for ASR or cleanup — never the default. TTS has no cloud path in this release.

> **v0.0.2** released. Binaries on [GitHub Releases](https://github.com/joe-broadhead/aurum/releases/tag/v0.0.2). Includes local TTS + `aurum-ffi`. Library API may change before `0.1.0`.

## Highlights

| | |
|--|--|
| **Local by default** | whisper.cpp STT + ONNX TTS — no API key |
| **Fast first run** | Quantized STT e.g. `tiny-q5_1` (~32 MB); TTS pack ~26 MB |
| **Embeddable** | PCM STT API · `aurum-ffi` · library TTS provider |
| **Cleanup / flow** | On-device rules or OpenRouter LLM (`aurum cleanup`) |
| **Local TTS** | `aurum tts` · 8 English voices · pinned SHA-256 pack |
| **Honest remote** | LLM-assisted OpenRouter ASR; unreliable SRT blocked by default |
| **Scriptable** | Exit codes · JSON metadata · pinned model integrity |

## 30-second install

```bash
git clone https://github.com/joe-broadhead/aurum.git
cd aurum
./scripts/install.sh # needs Rust 1.89+, cmake, ffmpeg

aurum models
aurum meeting.m4a --model tiny-q5_1
aurum meeting.m4a --cleanup clean
echo "um, hello there" | aurum cleanup -s clean
aurum tts "Hello from aurum" -O /tmp/hello.wav
```

## Workspace

| Crate | Role |
|-------|------|
| [`aurum-core`](crates/aurum-core) | Reusable library |
| [`aurum-stt`](crates/aurum) | CLI binary (`aurum`) |
| [`aurum-ffi`](crates/aurum-ffi) | C ABI for native embeds ([`aurum.h`](crates/aurum-ffi/include/aurum.h)) |

```toml
# Depend on the library (pin a commit or tag)
aurum-core = { git = "https://github.com/joe-broadhead/aurum", package = "aurum-core", tag = "v0.0.2" }
```

Full guide: [Library integration](https://joe-broadhead.github.io/aurum/library/integration/).

## CLI cheatsheet

```bash
aurum <FILE> [--model NAME] [-o txt|srt|json] [--cleanup STYLE]
aurum models
aurum cleanup [TEXT_FILE] --style clean   # alias: aurum flow
aurum tts "Hello" -O out.wav [--voice Luna]
aurum tts models && aurum tts voices
aurum <FILE> --provider openrouter --model google/gemini-2.5-flash-lite
```

| Flag | Default | Notes |
|------|---------|--------|
| `--provider` | `local` | `local` \| `openrouter` |
| `--model` | `base` (local) | e.g. `tiny-q5_1`, or an OpenRouter id |
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
| Models | [docs/guide/models.md](docs/guide/models.md) |
| Cleanup | [docs/guide/cleanup.md](docs/guide/cleanup.md) |
| TTS | [docs/guide/tts.md](docs/guide/tts.md) |
| Configuration | [docs/guide/configuration.md](docs/guide/configuration.md) |
| Native embeds (FFI) | [docs/library/ffi.md](docs/library/ffi.md) |
| Architecture | [docs/development/architecture.md](docs/development/architecture.md) |
| Security | [SECURITY.md](SECURITY.md) |

Site: **https://joe-broadhead.github.io/aurum/**

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

## Non-goals (v0.x)

Built-in microphone capture · speaker diarization · stable library API · Swift FFI.

## License

[MIT](LICENSE)
