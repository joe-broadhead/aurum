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
                                   
   Audio in. Text out.
   On-device by default.
```

<div align="center">

**Audio in. Text out. On-device by default.**
No API key required · clean providers · optional cleanup styles · reusable **`aurum-core`**

[Docs](https://joe-broadhead.github.io/aurum/) ·
[Quickstart](docs/getting-started/quickstart.md) ·
[Library](docs/library/integration.md) ·
[Cleanup](docs/guide/cleanup.md) ·
[Security](SECURITY.md)

</div>

---

## What it does

Aurum is an on-device speech-to-text **CLI** and **Rust library**:

1. Point it at an audio file (mp3, m4a, wav, flac, …)
2. Transcribe with **whisper.cpp** locally (Metal on macOS)
3. Optionally **clean** the text (fillers, bullets, professional, summary)
4. Emit **`txt`**, **`srt`**, or **`json`**

OpenRouter is available as an **optional** remote path for ASR or cleanup — never the default.

> **v0.0.0** released. Binaries on [GitHub Releases](https://github.com/joe-broadhead/aurum/releases/tag/v0.0.0). Library API may change before `0.1.0`.

## Highlights

| | |
|--|--|
| **Local by default** | whisper.cpp — no API key |
| **Fast first run** | Quantized models e.g. `tiny-q5_1` (~32 MB) |
| **Embeddable** | PCM-first API for mic hosts (`transcribe_pcm`, `PcmBuffer`) |
| **Cleanup / flow** | On-device rules or OpenRouter LLM (`aurum cleanup`) |
| **Honest remote** | LLM-assisted OpenRouter; unreliable SRT blocked by default |
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
```

## Workspace

| Crate | Role |
|-------|------|
| [`aurum-core`](crates/aurum-core) | Reusable library |
| [`aurum-stt`](crates/aurum) | CLI binary (`aurum`) |
| [`aurum-ffi`](crates/aurum-ffi) | C ABI for native embeds ([`aurum.h`](crates/aurum-ffi/include/aurum.h)) |

```toml
# Depend on the library (pin a commit or tag)
aurum-core = { git = "https://github.com/joe-broadhead/aurum", package = "aurum-core", tag = "v0.0.0" }
```

Full guide: [Library integration](https://joe-broadhead.github.io/aurum/library/integration/).

## CLI cheatsheet

```bash
aurum <FILE> [--model NAME] [-o txt|srt|json] [--cleanup STYLE]
aurum models
aurum cleanup [TEXT_FILE] --style clean # alias: aurum flow
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

## Non-goals (v0.0.0)

Built-in microphone capture · speaker diarization · stable library API · Swift FFI.

## License

[MIT](LICENSE)
