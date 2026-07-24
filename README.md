# Aurum

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.89%2B-orange.svg?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Docs](https://img.shields.io/badge/docs-mkdocs%20material-blue.svg?logo=materialformkdocs&logoColor=white)](https://joe-broadhead.github.io/aurum/)
[![CI](https://github.com/joe-broadhead/aurum/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/joe-broadhead/aurum/actions/workflows/ci.yml)

**Aurum** *(Latin: gold)* — local-first, cross-platform **transcription CLI** with a reusable Rust core for apps like ZephyrFlow.

- **Local by default** — whisper.cpp, no API key  
- **Clean provider trait** — OpenRouter optional (LLM-assisted)  
- **Scriptable output** — `txt` / `srt` / `json`  
- **Library + CLI** — `aurum-core` + `aurum` binary  

> Status: **v0.0.0** experimental · **public** · docs: <https://joe-broadhead.github.io/aurum/>  
> No crates.io / GitHub Release tag until explicitly approved. Release workflows are ready (prepare → merge → tag → binaries).

## 30-second path

```bash
git clone https://github.com/joe-broadhead/aurum.git
cd aurum
./Scripts/install.sh
aurum models
aurum meeting.m4a --model tiny-q5_1
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
aurum <AUDIO_FILE> [--provider local|openrouter] [--model NAME] [-o txt|srt|json]
aurum models
```

| Provider | Notes |
|----------|--------|
| `local` | whisper.cpp; Metal on macOS; model cache under platform cache dir |
| `openrouter` | LLM-assisted; `OPENROUTER_API_KEY`; SRT off by default |

## Docs

```bash
python3 -m venv .venv && .venv/bin/pip install -r docs/requirements.txt
.venv/bin/mkdocs serve
.venv/bin/mkdocs build --strict
```

Published site (when Pages enabled): <https://joe-broadhead.github.io/aurum/>

| | |
|--|--|
| Install | [docs/getting-started/installation.md](docs/getting-started/installation.md) |
| Quickstart | [docs/getting-started/quickstart.md](docs/getting-started/quickstart.md) |
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

Streaming mic, diarization, summarization, multi-remote providers, stable library API.

## License

MIT — see [LICENSE](LICENSE).
