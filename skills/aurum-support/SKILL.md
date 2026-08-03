---
name: aurum-support
description: "Create privacy-safe Aurum support bundles and file issues without leaking audio, transcripts, or API keys."
license: MIT
metadata:
  owner: "aurum"
  persona: "support"
  version: "0.0.23"
---

# Aurum support skill

## Mission

Help users report problems with **redacted diagnostics** only.

For reproducing STT/TTS failures first, load **`skills/aurum-speech/`** and
`aurum doctor`, then bundle.

## Flow

```bash
aurum doctor
aurum support-bundle -O aurum-support.json
# or: aurum support-bundle --stdout
```

Attach the JSON to a GitHub issue using the bug report template. Never attach raw audio or `.env` files.

## Redaction guarantees

Bundles exclude: API keys, audio, transcripts, private absolute home paths (tokenised).
