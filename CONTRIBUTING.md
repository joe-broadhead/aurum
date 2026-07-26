# Contributing to Aurum

Thanks for helping build on-device speech I/O that stays honest and embeddable.

## Ground rules

1. **On-device by default** — STT and TTS default paths work without an API key.
2. **Provider trait stays clean** — new backends implement `TranscriptionProvider`; do not special-case the CLI.
3. **Cleanup is separate** — ASR ≠ flow; implement `TextCleanup` for new cleanup backends.
4. **Actionable errors** — user / environment / provider taxonomy; no panics on expected failures.
5. **No silent network** — remote only when explicitly selected.
6. **Library consumers matter** — keep `aurum-core` minimal and documented.
7. **FFI stays narrow** — `aurum-ffi` is on-device PCM/preload/cancel/rules only; no cloud in the C ABI.
8. **No audit/spec dumps in git** — product docs under `docs/` only; long design specs stay out of the repo.

## Setup

```bash
git clone https://github.com/joe-broadhead/aurum.git
cd aurum
# Rust 1.89+, cmake, C/C++ toolchain, ffmpeg
cargo test --workspace --locked
cargo run -p aurum-stt -- tests/fixtures/sample.wav --model tiny-q5_1
cargo run -p aurum-stt -- tts "Hello" -O /tmp/a.wav --force
# optional: ./scripts/generate_tts_demos.sh
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
- Run `cargo fmt`, `clippy -D warnings`, `cargo test --workspace --locked`  
- Do not add dependencies that phone home by default  
- Do not publish crates or cut releases without maintainer approval  

## Code of conduct

Be respectful. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
