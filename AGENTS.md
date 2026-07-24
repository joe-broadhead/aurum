# AGENTS.md

Audio in. Text out. On-device by default.  
Stack: **Rust 1.89+**, Cargo workspace (`aurum-core`, `aurum-stt` CLI, `aurum-ffi`).  
Version source of truth: `VERSION` (must match workspace `version` and CHANGELOG).

## Layout

```text
crates/aurum-core/   # library engine
crates/aurum/        # CLI binary package (aurum-stt)
crates/aurum-ffi/    # C ABI façade for embedders
docs/                # MkDocs Material (user/product docs only)
tests/fixtures/      # sample audio
scripts/             # install, version_check, fixtures, publish dry-run
```

## Commands

```bash
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo build -p aurum-stt --release
cargo build -p aurum-ffi --release
cargo run -p aurum-stt -- tests/fixtures/sample.wav --model tiny-q5_1
cargo run -p aurum-stt -- models
cargo test -p aurum-core --test local_integration -- --ignored --nocapture
cargo test -p aurum-ffi --locked

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
- Call `clear_context_cache()` / `aurum_shutdown()` before process exit when using local whisper (Metal)
- Keep `aurum-ffi` narrow: façade only — no OpenRouter, no streaming theater

**Never commit**

- Audit reports, peer-review writeups, or design/spec documents (including drafts)
- One-off planning notes, scorecards, or “production grade spec” markdown
- Secrets, API tokens, or `.env` files with real credentials
- Large generated binding trees unless they are the maintained public surface

User-facing product docs under `docs/` (guides, install, architecture overview) are fine.  
Long-form design specs and audits stay **out of git** (local notes or issues only).

**Ask first**

- `git push`, force-push, tags, GitHub Releases
- Change repo visibility
- Publish to crates.io
- Add network-calling dependencies on the default path
- Cut a release without explicit user approval
- Expand the FFI ABI (treat as a stability boundary)
