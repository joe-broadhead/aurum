# AGENTS.md

Speech both ways. On-device by default.  
Stack: **Rust 1.89+**, Cargo workspace (`aurum-core`, `aurum-stt` CLI, `aurum-ffi`).  
Version source of truth: `VERSION` (must match workspace `version` and CHANGELOG).

## Agent skills

Load repository skills under `skills/` before improvising CLI flags:

| Skill | When |
|-------|------|
| `skills/aurum-cli/` | install, doctor, local STT/TTS, models, cleanup |
| `skills/aurum-batch/` | multi-file / resume batch transcription |
| `skills/aurum-embed/` | Rust library + C ABI jobs (provisional) |
| `skills/aurum-support/` | privacy-safe support bundles and issue reports |

## Layout

```text
crates/aurum-core/   # library engine (STT + optional TTS feature)
crates/aurum/        # CLI binary package (aurum-stt)
crates/aurum-ffi/    # C ABI façade (STT, cleanup, local TTS jobs; ABI v2)
docs/                # MkDocs Material (user/product docs only)
skills/              # agent skill packs (SKILL.md + references/)
evals/               # smoke corpus + synthetic audio
tests/fixtures/      # sample audio (STT)
scripts/             # install, version_check, fixtures, tts demos, publish dry-run
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
cargo run -p aurum-stt -- models recommend --profile balance
cargo run -p aurum-stt -- tts "Hello from aurum" -O /tmp/a.wav --force
cargo run -p aurum-stt -- tts models
cargo run -p aurum-stt -- tts voices
cargo run -p aurum-stt -- batch tests/fixtures -O /tmp/aurum-batch --dry-run
cargo run -p aurum-stt -- support-bundle --stdout
cargo run -p aurum-stt -- completions zsh
cargo run -p aurum-stt -- man | head
cargo test -p aurum-core --test local_integration -- --ignored --nocapture
cargo test -p aurum-core --test tts_synth -- --ignored --nocapture
cargo test -p aurum-ffi --locked
./scripts/generate_tts_demos.sh
./scripts/generate_cli_reference.sh
./scripts/check_security_tool_pins.sh

python3 -m venv .venv && .venv/bin/pip install -r docs/requirements.txt
.venv/bin/mkdocs build --strict

./scripts/version_check.sh
```

## Boundaries

**Always do**

- Keep on-device defaults (no API key required) for STT **and** TTS
- Preserve provider + cleanup + TTS module splits
- Actionable errors; no panics on expected failure paths
- Prefer small, focused diffs
- Call `clear_context_cache()` / `aurum_shutdown()` before process exit when using local whisper (Metal)
- Treat `aurum-ffi` as a **provisional** embed surface: STT, rules cleanup, **and** local TTS jobs (ABI v2); no OpenRouter, no mic ownership
- Pin model/voice downloads; fail closed on integrity mismatch
- TTS: MIT-safe default path (no GPL phonemizer); document model licenses
- Never invent CLI flags — use `aurum --help` / generated reference

**Never commit**

- Audit reports, peer-review writeups, or design/spec documents (including drafts)
- One-off planning notes, scorecards, or “production grade spec” markdown
- Contest result dumps (`CONTEST_RESULT.md`, etc.)
- Secrets, API tokens, or `.env` files with real credentials
- Generated TTS demo WAVs or large model binaries
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
- Add GPL-linked dependencies on the default binary path
