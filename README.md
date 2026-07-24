# Aurum

<div align="center">

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.89%2B-orange.svg?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Docs](https://img.shields.io/badge/docs-mkdocs%20material-blue.svg?logo=materialformkdocs&logoColor=white)](https://joe-broadhead.github.io/aurum/)
[![CI](https://github.com/joe-broadhead/aurum/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/joe-broadhead/aurum/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/joe-broadhead/aurum?include_prereleases&logo=github)](https://github.com/joe-broadhead/aurum/releases)

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

**Audio in. Text out. On-device by default.** No API key required.  
Clean provider trait · Zephyr-style cleanup · reusable **`aurum-core`** for apps like ZephyrFlow.

[Docs](https://joe-broadhead.github.io/aurum/) · [Quickstart](docs/getting-started/quickstart.md) · [Library](docs/library/integration.md) · [Cleanup](docs/guide/cleanup.md)

</div>

---

## What it does

Aurum is an **on-device speech-to-text CLI** and Rust library:

1. Point it at an audio file (mp3, m4a, wav, …)
2. It runs **whisper.cpp** locally (Metal on macOS)
3. Optional **cleanup** (clean / bullets / professional / summary) — on-device rules or OpenRouter
4. Emit `txt`, `srt`, or `json`

> Status: **v0.0.0** experimental · public · docs live.  
> No crates.io / GitHub Release **tag** until explicitly approved. Release workflows are ready.

## Highlights

- **Local by default** — whisper.cpp, no API key  
- **Quantized models** — e.g. `tiny-q5_1` (~32 MB) for a fast first run  
- **PCM embedder API** — mic hosts skip files/ffmpeg (`transcribe_pcm`, `PcmBuffer`)  
- **Cleanup / flow** — Zephyr-style styles; `aurum cleanup` on stdin/text  
- **OpenRouter optional** — LLM-assisted ASR or cleanup (never default)  
- **Scriptable** — exit codes, JSON metadata (`backend_kind`, `cleanup_style`, …)

## 30-second path

```bash
git clone https://github.com/joe-broadhead/aurum.git
cd aurum
./scripts/install.sh
aurum models
aurum meeting.m4a --model tiny-q5_1
aurum meeting.m4a --cleanup clean
echo "um, hello there" | aurum cleanup -s clean
```

Prerequisites: **Rust 1.89+**, **cmake**, C/C++ toolchain, **ffmpeg**.

## Workspace

| Crate | Role |
|-------|------|
| [`aurum-core`](crates/aurum-core) | Reusable library |
| [`aurum`](crates/aurum) | CLI binary |

### Use `aurum-core` from another project

```toml
aurum-core = { git = "https://github.com/joe-broadhead/aurum", package = "aurum-core", rev = "PIN_ME" }
# or path = "../aurum/crates/aurum-core"
```

See [Library integration](https://joe-broadhead.github.io/aurum/library/integration/).

## CLI

```bash
aurum <AUDIO_FILE> [--provider local|openrouter] [--model NAME] [-o txt|srt|json] [--cleanup STYLE]
aurum models
aurum cleanup [TEXT_FILE] --style clean   # also: aurum flow
```

| Provider | Notes |
|----------|--------|
| `local` | whisper.cpp; Metal on macOS; models under platform cache |
| `openrouter` | LLM-assisted; `OPENROUTER_API_KEY`; SRT off by default |

## Docs

```bash
python3 -m venv .venv && .venv/bin/pip install -r docs/requirements.txt
.venv/bin/mkdocs serve
.venv/bin/mkdocs build --strict
```

| | |
|--|--|
| Install | [docs/getting-started/installation.md](docs/getting-started/installation.md) |
| Quickstart | [docs/getting-started/quickstart.md](docs/getting-started/quickstart.md) |
| Cleanup | [docs/guide/cleanup.md](docs/guide/cleanup.md) |
| Architecture | [docs/development/architecture.md](docs/development/architecture.md) |
| Release | [docs/development/release.md](docs/development/release.md) |
| Security | [SECURITY.md](SECURITY.md) |

## Development

```bash
make ci                 # fmt + clippy + test + version-check
make build              # release CLI
cargo test --workspace --locked
```

## Release (maintainer)

Do **not** tag without approval. Flow mirrors ZephyrFlow:

1. Bump `VERSION` + workspace version + `CHANGELOG`  
2. `workflow_dispatch` → **Prepare Release**  
3. Merge `release/x.y.z` PR  
4. Tag workflow creates `vX.Y.Z` and dispatches multi-platform binary build  

See [docs/development/release.md](docs/development/release.md).

## Non-goals (v0.0.0)

Built-in mic capture, diarization, stable library API guarantees, Swift FFI.

## License

MIT — see [LICENSE](LICENSE).
