---
name: aurum-embed
description: "Embed Aurum via aurum-core (Rust) or aurum-ffi (C ABI v2) for local STT, cleanup, and TTS jobs. Contracts are provisional."
license: MIT
metadata:
  owner: "aurum"
  persona: "embed"
  version: "0.0.4"
---

# Aurum embed skill

## Mission

Integrate Aurum into host applications **without shelling out**, using local-only defaults.

## Rules

- Prefer `local_only` in host configs for production embeds.
- ABI v2: jobs for preload/transcribe/cleanup/TTS; destroy/drain ownership rules apply.
- Call process shutdown / clear whisper cache before exit on Metal.
- Do not expose OpenRouter credentials through the FFI surface (not supported).
- Treat public Rust structs and ABI as **provisional** until 1.0.

## Load order

- `references/ffi.md` for C surface
- `docs/library/ffi.md` and `docs/library/integration.md` in-repo for full detail
