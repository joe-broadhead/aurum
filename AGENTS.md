# AGENTS.md

Speech both ways. On-device by default.  
Stack: **Rust 1.89+**, Cargo workspace (`aurum-core`, `aurum-stt` CLI, `aurum-ffi`).  
Version source of truth: `VERSION` (must match workspace `version` and CHANGELOG).  
Product tip: **0.0.22**. Continuous **0.0.x** line; product-outcomes cut **0.0.22** (not 1.0).

## Agent skills

Load repository skills under `skills/` **before** improvising CLI flags or
provider/model IDs:

| Skill | When |
|-------|------|
| **`skills/aurum-speech/`** | **Primary for all STT and TTS** — local + remote providers, models/voices, batch, cleanup, doctor/cache, embed boundaries |
| `skills/aurum-cli/` | install, first-run verify, doctor only (defers speech details to aurum-speech) |
| `skills/aurum-batch/` | multi-file / resume batch transcription details |
| `skills/aurum-embed/` | Rust library + C ABI jobs (provisional; local FFI only) |
| `skills/aurum-support/` | privacy-safe support bundles and issue reports |

Never invent flags: run `aurum --help` / `aurum tts --help` or read
`docs/reference/cli-help.md` and `docs/guide/provider-matrix.md`.

## Layout

```text
crates/aurum-core/   # library engine (STT + TTS + providers; TTS feature default on)
crates/aurum/        # CLI binary package (aurum-stt → binary `aurum`)
crates/aurum-ffi/    # C ABI façade (local STT, cleanup, local TTS jobs; ABI v2)
docs/                # MkDocs Material (user/product docs)
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

# STT / TTS smokes (see skills/aurum-speech/)
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
- Remote providers only via explicit `--provider` + matching secret (`openrouter`, `openai`, `elevenlabs`, `xai`)
- Preserve provider + cleanup + TTS module splits
- Actionable errors; no panics on expected failure paths
- Prefer small, focused diffs
- Call `clear_context_cache()` / `aurum_shutdown()` before process exit when using local whisper (Metal)
- Treat `aurum-ffi` as a **provisional** embed surface: local STT, rules cleanup, **local** TTS jobs (ABI v2); **no remote** on FFI, no mic ownership
- Pin model/voice downloads; fail closed on integrity mismatch
- TTS: MIT-safe default path (no GPL phonemizer); document model licenses
- Never invent CLI flags or unreviewed model IDs — use help + provider matrix
- Load `skills/aurum-speech/` for any STT/TTS task

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
