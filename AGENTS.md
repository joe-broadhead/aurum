# AGENTS.md

Audio in. Text out. On-device by default.  
Stack: **Rust 1.89+**, Cargo workspace (`aurum-core` + `aurum` binary).  
Version source of truth: `VERSION` (must match workspace `version` and CHANGELOG).

## Layout

```text
crates/aurum-core/   # library
crates/aurum/        # CLI binary
docs/                # MkDocs Material
tests/fixtures/      # sample audio
scripts/             # install, version_check, fixtures, publish dry-run
```

## Commands

```bash
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo build -p aurum-stt --release
cargo run -p aurum-stt -- tests/fixtures/sample.wav --model tiny-q5_1
cargo run -p aurum-stt -- models
cargo test -p aurum-core --test local_integration -- --ignored --nocapture

python3 -m venv .venv && .venv/bin/pip install -r docs/requirements.txt
.venv/bin/mkdocs build --strict

./scripts/version_check.sh
```

## Boundaries

**Always do**

- Keep on-device defaults (no API key required)
- Preserve provider + cleanup trait splits
- Actionable errors; no panics on expected failure paths
- Prefer small, focused diffs
- Call `clear_context_cache()` before process exit when using local whisper as a library (Metal)

**Ask first**

- `git push`, force-push, tags, GitHub Releases
- Change repo visibility
- Publish to crates.io
- Add network-calling dependencies on the default path
- Cut a release without explicit user approval
