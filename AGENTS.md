# AGENTS.md

Speech both ways. On-device by default.  
Stack: **Rust 1.89+**, Cargo workspace (`aurum-core`, `aurum-stt` CLI, `aurum-ffi`).  
Version source of truth: `VERSION` (must match workspace `version` and CHANGELOG).  
Product tip: **0.0.23**. Continuous **0.0.x** line; correctness patch **0.0.23** after product-outcomes cut **0.0.22** (not 1.0).

## Cloud Agent environment

Repo-managed config: `.cursor/environment.json` (Dockerfile + `bash .cursor/install.sh`).

Install is idempotent. It installs CI Linux packages (`build-essential`, `cmake`, `pkg-config`, `libasound2-dev`, `ffmpeg`) plus `libstdc++-14-dev` (needed when `/usr/bin/c++` is clang, as on Cursor’s default image), ensures Rust **stable** with `rustfmt`/`clippy` plus MSRV **1.89.0**, runs `cargo fetch --locked`, creates `.venv` for docs, and installs pinned `cargo-audit@0.22.2` / `cargo-deny@0.19.0` (same pins as the CI security job).

No long-running `start` services are required for normal development.

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
.cursor/             # Cloud Agent Dockerfile + install bootstrap
```

## Commands

### Fast subset (run first on every change)

Matches the CI **lint** job plus the default **test** job (Linux):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/version_check.sh
./scripts/check_action_pins.sh
./scripts/check_crates_publish_policy.sh
cargo test --workspace --locked
```

Optional quick compile checks used elsewhere in CI:

```bash
cargo check -p aurum-core --no-default-features --locked   # STT-only feature gate
cargo +1.89.0 check --workspace --locked                   # MSRV
```

### Full local CI-shaped suite (when touching the named area)

```bash
# Release builds (CI test job)
cargo build -p aurum-stt --release --locked
cargo build -p aurum-ffi --release --locked --no-default-features
# Native C11/C++17 examples (Linux; see ci.yml)
cc -std=c11 -I crates/aurum-ffi/include crates/aurum-ffi/examples/job_cleanup.c \
  target/release/libaurum_ffi.a -lpthread -ldl -lm -lstdc++ -o /tmp/aurum_job_cleanup
c++ -std=c++17 -I crates/aurum-ffi/include crates/aurum-ffi/examples/engine_raii.cpp \
  target/release/libaurum_ffi.a -lpthread -ldl -lm -lstdc++ -o /tmp/aurum_engine_raii

# Security job
./scripts/check_security_tool_pins.sh
cargo audit
cargo deny check

# Docs job (Python 3.12; .venv created by .cursor/install.sh)
.venv/bin/mkdocs build --strict

# Provider smoke dry-run (no credentials)
AURUM_SMOKE_SKIP_RELEASE_GATE=1 ./scripts/protected_provider_smoke.sh --dry-run --out /tmp/aurum-provider-smoke
```

### Product smokes (optional; may download models)

```bash
cargo run -p aurum-stt -- tests/fixtures/sample.wav --model tiny-q5_1
cargo run -p aurum-stt -- models
cargo run -p aurum-stt -- models recommend --profile balance
cargo run -p aurum-stt -- tts "Hello from aurum" -O /tmp/a.wav --force
cargo run -p aurum-stt -- converse tests/fixtures/sample.wav --reply-text "Hello" -O /tmp/a.wav --force
cargo run -p aurum-stt -- tts models
cargo run -p aurum-stt -- tts voices
cargo run -p aurum-stt -- batch tests/fixtures -O /tmp/aurum-batch --dry-run
cargo run -p aurum-stt -- support-bundle --stdout
cargo run -p aurum-stt -- completions zsh
cargo run -p aurum-stt -- man | head
```

### Makefile shortcuts

`make fmt`, `make lint`, `make test`, `make build`, `make ci` (fmt + lint + test + version-check).

### Tests by segment

The suite is declared in `test-segments.toml`; do not invent a test command.

```bash
scripts/test.sh --list              # every segment, with cost and requirements
scripts/test.sh                     # the default set (fast, offline)
scripts/test.sh --full              # every non-nightly segment except graphical
scripts/test.sh --segment test-core # exactly one segment
scripts/test.sh --verify            # every test in exactly one segment
```

`--verify` proves 604 tests: 600 ordinary tests across `test-core`, `test-cli`,
`test-ffi`, and `test-doc`, plus the four `#[ignore]` tests in `test-ignored`
(which skip unless `AURUM_INTEGRATION` and `AURUM_TTS_INTEGRATION` are set). A
new test is not verified until it appears in one of those segments.

## Tests that need special infrastructure (skip in default Cloud VMs)

| Suite | Why it cannot run here by default | How to skip / gate |
|-------|-----------------------------------|--------------------|
| `local_integration` (`#[ignore]`) | Downloads whisper `tiny-q5_1`; CI runs it on **macOS** with Metal (`AURUM_INTEGRATION=1`) | Omit `--ignored`; do not set `AURUM_INTEGRATION` |
| `tts_synth` / `kokoro_real_synth` (`#[ignore]`) | Downloads TTS voice packs on first run | Omit `--ignored`; do not set `AURUM_TTS_INTEGRATION` |
| Provider **live** smokes / catalogue probes | Need `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ELEVENLABS_API_KEY`, `XAI_API_KEY` (and related) | Use `--dry-run` only; never invent credentials |
| Fuzz smoke (`fuzz/`) | Needs **nightly** + `cargo-fuzz@0.12.0` | Skip unless the ticket requires fuzz work |
| Miri / ASan / UBSan / mutants / coverage | Extra toolchains or long runtimes (`scripts/run_miri.sh`, `run_sanitizers.sh`, `run_mutants.sh`, `coverage_trust.sh`) | Skip unless the ticket names them |
| macOS / Windows matrix | Cloud Agent VMs are Linux | Rely on GitHub Actions for those OSes |
| GPU / Metal paths | No Apple Metal or dedicated GPU in default cloud VMs | Local whisper on Linux uses CPU; Metal teardown notes apply to macOS hosts only |
| Release publish / crates.io / GitHub Releases | Credentials + maintainer approval | Never run publish/release workflows from agents |

Default `cargo test --workspace --locked` already skips `#[ignore]` tests. That is the expected cloud default.

## Conventions

- **Commits**: conventional style used on `master` (`fix:`, `chore:`, `docs:`, …). Keep messages short and imperative.
- **DCO / sign-off**: not required by CI or `CONTRIBUTING.md` (no Signed-off-by gate).
- **Branches**: one focused branch per ticket/PR. Cloud agents use `cursor/<short-description>-<id>` when that template is supplied.
- **Changelog**: update `CHANGELOG.md` under `[Unreleased]` for user-visible changes.
- **PRs**: keep focused; fill `.github/pull_request_template.md`. Do not merge.

## Ticket / write-set rules (Cloud Agents)

- **One Linear ticket per PR.** Do not bundle unrelated fixes.
- **Stay inside the ticket’s named write set.** If a change is required outside it, stop and report rather than expanding scope.
- **Never merge** PRs (humans merge).
- **Never publish or release** (no crates.io, tags, GitHub Releases, or docs deploy).
- **Never weaken tests or CI** (no deleting assertions, skipping gates, or relaxing pins to “make green”).
- **Never commit secrets** (no `.env` with real keys, tokens, or credentials). Use `.env.example` patterns only.
- Prefer small, focused diffs. Do not change product code except trivial fixes strictly required for verified tests to run — and document those.

## Boundaries

**Always do**

- Keep on-device defaults (no API key required) for STT **and** TTS
- Remote providers only via explicit `--provider` + matching secret (`openrouter`, `openai`, `elevenlabs`, `xai`)
- Preserve provider + cleanup + TTS module splits
- Actionable errors; no panics on expected failure paths
- Prefer small, focused diffs
- Call `clear_context_cache()` / `aurum_shutdown()` before process exit when using local whisper (Metal)
- Treat `aurum-ffi` as a **provisional** embed surface: local STT, rules cleanup, **local** TTS jobs (ABI v2); **no remote** on FFI, no mic ownership
- Session/voice hosts use `aurum converse --stdio` (host-owned paths). Do not put LLM or mic ownership in `aurum-core`
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
