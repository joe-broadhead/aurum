---
name: aurum-cli
description: "Install, verify, and run Aurum local STT/TTS/cleanup via the aurum CLI. Use for transcription, synthesis, models, doctor, profiles, and first-run setup. Prefer local-first defaults; never invent flags."
license: MIT
metadata:
  owner: "aurum"
  persona: "cli"
  version: "0.0.4"
---

# Aurum CLI skill

## Mission

Help a developer **install Aurum, run local STT and TTS, list models, and diagnose** without reading the whole repository or inventing CLI surface.

## First principles

1. **Local-first** — default STT and TTS are on-device; remote STT is explicit.
2. **Do not invent flags** — run `aurum --help` / subcommand `--help` or load `references/`.
3. **Verified models** — catalogue models download with integrity checks; fail closed on mismatch.
4. **Privacy** — do not paste API keys, audio, or full transcripts into chat unless the user asks.
5. **Default model stays `base`** until evidence reviews change it; profiles are opt-in.

## Deterministic flow

1. Verify install: `aurum --version` and `aurum doctor`.
2. List models: `aurum models` (optional: `aurum models recommend --profile balance`).
3. Local STT: `aurum path/to/audio.wav` or `--model tiny-q5_1` for a small trial.
4. Local TTS: `aurum tts "Hello" -O /tmp/out.wav --force`.
5. Cleanup text: `echo 'um hello' | aurum cleanup --style clean`.
6. On failure: `aurum doctor --json` or `aurum support-bundle` (see `aurum-support` skill).

## Load order

- Read `references/install-and-verify.md` for install paths.
- Read `references/stt-local.md` / `references/tts-local.md` for command shapes.
- Read `references/remote-explicit.md` only when the user requests OpenRouter/remote.
- Read `references/do-not.md` before any network or credential advice.

## Output standard

- Show the exact command used.
- Prefer local models; if remote is used, say so explicitly.
- Never claim quality/performance numbers without a retained eval/bench report.
