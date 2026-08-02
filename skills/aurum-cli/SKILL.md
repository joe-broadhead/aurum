---
name: aurum-cli
description: "Install and verify the Aurum CLI (doctor, version, first-run). For all STT/TTS work load skills/aurum-speech instead. Prefer local-first defaults; never invent flags."
license: MIT
metadata:
  owner: "aurum"
  persona: "cli"
  version: "0.0.22"
---

# Aurum CLI skill (install / verify)

## Mission

Help a developer **install Aurum and confirm it runs**.  
For **transcription or synthesis**, load **`skills/aurum-speech/`** — that pack is
the authority for STT/TTS providers, models, remote keys, batch, and cleanup.

## First principles

1. **Local-first** — default STT/TTS need no API key.
2. **Do not invent flags** — `aurum --help` or `docs/reference/cli-help.md`.
3. **Privacy** — do not paste API keys, audio, or full transcripts into chat unless asked.
4. **Speech tasks → aurum-speech** — do not duplicate provider matrices here.

## Deterministic flow

1. Install: see `references/install-and-verify.md`.
2. Verify: `aurum --version` and `aurum doctor`.
3. If the user wants STT or TTS: **load `skills/aurum-speech/`** and follow it.
4. On failure: `aurum doctor --json` or `aurum support-bundle` (`skills/aurum-support/`).

## Load order

- `references/install-and-verify.md` — install paths
- `references/do-not.md` — hard limits
- For speech: **`../aurum-speech/SKILL.md`** (and its references)

## Output standard

- Show the exact install/verify commands used.
- Point speech work at `aurum-speech` rather than improvising remote flags here.
