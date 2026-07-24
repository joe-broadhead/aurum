# AGENTS.md

Audio in. Text out. On-device by default.  
Stack: **Rust 1.89+**, Cargo workspace (`aurum-core` + `aurum` binary).  
Version source of truth: `VERSION` (must match workspace `version` and CHANGELOG).

Soft fallback name if needed: `aurum-stt`.

## Layout

```text
crates/aurum-core/   # library (providers, audio, models, output)
crates/aurum/        # CLI binary
docs/                # MkDocs Material
tests/fixtures/      # sample audio
scripts/             # release / install helpers
```

## Commands

```bash
cargo test --workspace --locked
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build -p aurum --release
cargo run -p aurum -- tests/fixtures/sample.wav --model tiny-q5_1
cargo run -p aurum -- models
cargo test -p aurum-core --test local_integration -- --ignored --nocapture

# Docs
python3 -m venv .venv && .venv/bin/pip install -r docs/requirements.txt
.venv/bin/mkdocs build --strict

# Version gate (same idea as CI)
v=$(tr -d '[:space:]' < VERSION)
grep -q "^version = \"$v\"" Cargo.toml
grep -q "^## \\[$v\\]" CHANGELOG.md
```

## Boundaries

**Always do**
- Keep local-first defaults (no API key required)
- Preserve provider trait abstraction
- Actionable errors; no panics on expected failure paths
- Prefer small, focused diffs
- Call `clear_context_cache()` before process exit when using the local provider as a library (Metal)

**Ask first**
- `git push`, force-push, tags, GitHub Releases
- Change repo visibility
- Publish to crates.io
- Add network-calling dependencies on the default path
- Cut a release without explicit user approval
