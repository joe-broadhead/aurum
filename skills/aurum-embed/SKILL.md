---
name: aurum-embed
description: "Embed Aurum via aurum-core (Rust) or aurum-ffi (C ABI v2) for local STT, cleanup, and local TTS jobs. Remote speech is library/CLI only — not on the C ABI. Contracts are provisional on 0.0.x."
license: MIT
metadata:
  owner: "aurum"
  persona: "embed"
  version: "0.0.22"
---

# Aurum embed skill

## Mission

Integrate Aurum into host applications **without shelling out**, using local-only
defaults on the C ABI and deliberate provider selection in Rust.

For CLI STT/TTS recipes, load **`skills/aurum-speech/`**.

## Rules

- Prefer `local_only` in host configs for production embeds that must not network.
- Prefer `AurumEngine` over process-global whisper/TTS constructors in long-lived hosts.
- ABI v2: jobs for preload/transcribe/cleanup/**local** TTS; destroy/drain ownership rules apply.
- Call process shutdown / clear whisper cache before exit on Metal.
- **Do not** expose remote credentials through the FFI surface (remote **not supported** on C ABI).
- Treat public Rust APIs and ABI as **provisional** on continuous **0.0.x** (pin `0.0.22` / tag `v0.0.22`; continuous 0.0.x, not 1.0).

## Load order

- `references/ffi.md` for C surface
- `../aurum-speech/references/embed.md` for engine + provider notes
- `docs/library/ffi.md`, `docs/library/engine.md`, `docs/library/integration.md` for full detail
