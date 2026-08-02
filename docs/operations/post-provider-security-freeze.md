# Post-provider security freeze checklist (JOE-1981)

**Purpose:** cut **one** immutable candidate covering the provider platform +
remediation (JOE-1975–1980, F-005 snap staging), then hand to JOE-1920 human review.

**Historical freeze:** `v0.0.18` remains the **pre-provider** security freeze only.

## Do not

- Overwrite or re-point `v0.0.18`
- Represent green PR CI alone as independent security review
- Mark JOE-1655 Done without residual closeout ([v0021-residual-closeout.md](v0021-residual-closeout.md))
- Tag without explicit maintainer approval

## Freeze prep (maintainer)

**Historical candidate VERSION:** `0.0.19` (shipped as post-provider freeze; JOE-1920 signed off).
Current published tip at docs refresh: **`0.0.20`**. Next assurance cut: **`0.0.21`**.

1. Ensure `master` is clean and contains the remediation merges (1975–1980 + F-005 snap).
2. Record exact `git rev-parse HEAD` (full 40 hex).
3. Align `VERSION`, workspace crate versions, and CHANGELOG Unreleased notes for the cut.
4. Run locally (clean tree):
   ```bash
   cargo test --workspace --locked
   cargo test -p aurum-core --lib remote:: provider_platform::
   ./scripts/rc_freeze_check.sh
   ```
5. Optional release rehearsal (no publish):
   ```bash
   ./scripts/generate_sbom.sh dist/sbom
   ./scripts/generate_rc_exit_report.sh
   ```
6. Propose tag name on the 0.0.x line (e.g. `v0.0.19`) — **wait for human GO**. Do not invent a parallel `1.0.0` tag for this programme.
7. After tag: independent clean clone of **that tag only** for retest.

## Independent retest recipes (clean clone + Cargo)

| Finding | Recipe (redacted results only) |
|---------|--------------------------------|
| F-001 | Synthetic keys for openrouter/openai/elevenlabs/xai through doctor/support/JSON; canary matrix |
| F-002 | `verify` publish policy inject/restore on disposable worktree |
| F-005 | `stage_verified_isolates_from_source_swap` + pack symlink tests |
| F-006 | `verify_release_assets.sh` with full 40-hex expect; wrong-commit must fail |
| JOE-1975 | `remote::interrupt` cancel/deadline/permit isolation tests |
| JOE-1976 | xAI mock `/v1/stt` + `/v1/tts`; reject OpenAI paths/voices |
| JOE-1977 | `remote::wire_format` matrix |
| JOE-1978–1980 | OpenRouter exact default; ProviderId serde; SecretString Debug; public_network_reason |
| Local-only | Synthetic keys present; `local_only` + default local; zero network expectation |

## Provider tier honesty (at freeze)

| Provider | Tier claim |
|----------|------------|
| local | supported |
| openrouter STT | supported (reviewed models) |
| openrouter TTS | **experimental** until protected smoke |
| openai | supported (reviewed models; protected smoke optional) |
| elevenlabs | supported (reviewed models; protected smoke optional) |
| xai | **experimental** (official REST; protected smoke pending) |

## Human sign-off

JOE-1920 only: qualified independent human on the **tagged** candidate.
Automation may prepare evidence; it may not self-approve the assurance cut
(v0.0.19 freeze; subsequent **v0.0.21** programme GO).
