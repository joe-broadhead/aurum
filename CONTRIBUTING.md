# Contributing to Aurum

Thanks for helping build a clean, on-device speech-to-text foundation.

## Ground rules

1. **Local-first** — default path must work without an API key.
2. **Provider trait stays clean** — new backends implement `TranscriptionProvider`; do not special-case the CLI.
3. **Actionable errors** — user / environment / provider taxonomy; no panics on expected failures.
4. **No silent network** — remote providers only when explicitly selected.
5. **Library consumers matter** — `aurum-core` is for ZephyrFlow and others; keep the API minimal and documented.

## Setup

```bash
git clone https://github.com/joe-broadhead/aurum.git
cd aurum
# Prerequisites: Rust 1.89+, cmake, C/C++ toolchain, ffmpeg
cargo test --workspace --locked
cargo run -p aurum -- tests/fixtures/sample.wav --model tiny-q5_1
```

## Docs

```bash
python3 -m venv .venv
.venv/bin/pip install -r docs/requirements.txt
.venv/bin/mkdocs serve
.venv/bin/mkdocs build --strict
```

## Pull requests

- Keep PRs focused
- Update `CHANGELOG.md` under `[Unreleased]` when user-visible
- Run `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`
- Do not add dependencies that phone home by default
- Do not publish crates or cut releases without maintainer approval

## Code of conduct

Be respectful. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
