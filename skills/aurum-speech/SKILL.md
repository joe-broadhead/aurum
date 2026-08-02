---
name: aurum-speech
description: "Use Aurum end-to-end for speech-to-text (STT) and text-to-speech (TTS): local whisper/Kitten defaults, opt-in remote providers (OpenRouter, OpenAI, ElevenLabs, xAI), batch, cleanup, doctor, cache, and embed paths. Load this skill before running or recommending STT/TTS commands. Never invent flags or model IDs."
license: MIT
metadata:
  owner: "aurum"
  persona: "speech"
  version: "0.0.20"
  product_tip: "0.0.20"
  next_assurance_cut: "0.0.21"
---

# Aurum speech skill (STT + TTS)

## Mission

Teach an agent to **run and recommend correct Aurum STT and TTS** using the full
product surface — CLI, config, batch, doctor/cache, library, and FFI — without
inventing flags, over-claiming providers, or leaking secrets.

If you only need install/doctor basics, see also `skills/aurum-cli/`.  
For multi-file resume only, see `skills/aurum-batch/`.  
For host embeds only, see `skills/aurum-embed/`.  
**This skill is the speech authority** when the task is “transcribe / synthesize.”

## First principles (always)

1. **Local-first** — STT default `local` (whisper.cpp); TTS default `local` (Kitten ONNX). No API key required.
2. **Remote is opt-in** — requires explicit `--provider` **and** the matching env key. Presence of a key never changes the default provider.
3. **Do not invent** — flags, config keys, model IDs, or voice IDs. Prefer:
   - `aurum --help` / `aurum tts --help` / `aurum models` / `aurum tts models` / `aurum tts voices`
   - `docs/reference/cli-help.md` (generated snapshot)
   - `docs/guide/provider-matrix.md` (reviewed catalogues)
4. **Honesty** — report `provider`, model, and when timestamps are unreliable. Do not claim WER/RTF without a retained eval report.
5. **Privacy** — never echo API keys, full audio, or full transcripts into chat unless the user explicitly wants that content.
6. **Versioning** — product is continuous **0.0.x** (tip **0.0.20**); next assurance cut **0.0.21**, not 1.0. Pin tags/crates versions in dependents.

## Binary and workspace names

| What users type | Package / crate |
|-----------------|-----------------|
| `aurum` CLI | crates.io `aurum-stt` |
| Library | `aurum-core` |
| C ABI | `aurum-ffi` (`aurum.h`, ABI v2) |

From a clone:

```bash
cargo run -p aurum-stt -- <args…>
# or installed:
aurum <args…>
```

## Decision tree (pick a path)

```text
Need speech?
├─ Audio → text  → STT
│  ├─ Default / offline / privacy → local whisper
│  ├─ Cloud OpenAI ASR            → --provider openai
│  ├─ Cloud OpenRouter ASR/chat   → --provider openrouter
│  ├─ Cloud xAI (experimental)    → --provider xai
│  └─ Many files                  → aurum batch (+ local or explicit remote)
└─ Text → audio  → TTS
   ├─ Default / offline           → local Kitten (or --model kokoro-82m-int8)
   ├─ OpenRouter speech           → --provider openrouter
   ├─ OpenAI speech               → --provider openai
   ├─ ElevenLabs                  → --provider elevenlabs (+ voice_id)
   └─ xAI (experimental)          → --provider xai
```

Full matrices: `references/stt.md`, `references/tts.md`.  
Tooling (doctor, cache, cleanup, batch): `references/tooling.md`.  
Hard limits: `references/do-not.md`.  
Embed: `references/embed.md`.

## Canonical local smoke (no keys)

```bash
aurum doctor
aurum models
aurum tests/fixtures/sample.wav --model tiny-q5_1
aurum tts "Hello from aurum" -O /tmp/hello.wav --force --emit-json
aurum tts models && aurum tts voices
```

From repo without install:

```bash
cargo run -p aurum-stt --release -- tests/fixtures/sample.wav --model tiny-q5_1
cargo run -p aurum-stt --release -- tts "Hello from aurum" -O /tmp/hello.wav --force
```

## Canonical remote smoke (only if user asked + key present)

Export keys in the shell; **do not print them**.

```bash
# STT
export OPENAI_API_KEY=…          # openai
aurum talk.mp3 --provider openai --model whisper-1 -o json

export OPENROUTER_API_KEY=…      # openrouter (reviewed ASR preferred)
aurum talk.mp3 --provider openrouter --model openai/whisper-large-v3 -o srt

export XAI_API_KEY=…             # xai experimental
aurum talk.mp3 --provider xai --model xai-stt

# TTS (provider-aware defaults when only --provider is set)
export OPENROUTER_API_KEY=…
aurum tts "Hello" --provider openrouter -O /tmp/or.wav --force --emit-json

export OPENAI_API_KEY=…
aurum tts "Hello" --provider openai --model tts-1 --voice alloy -O /tmp/oai.wav --force

export ELEVENLABS_API_KEY=…
aurum tts "Hello" --provider elevenlabs \
  --model eleven_multilingual_v2 --voice 21m00Tcm4TlvDq8ikWAM -O /tmp/el.wav --force

export XAI_API_KEY=…
aurum tts "Hello" --provider xai --model xai-tts --voice eve -O /tmp/x.wav --force
```

OpenRouter privacy: if every call fails with guardrail/privacy errors, the user
must fix https://openrouter.ai/settings/privacy — do not “fix” by inventing models.

## Output contract for agents

When you run or recommend a speech command, report:

| Field | Example |
|-------|---------|
| Direction | STT or TTS |
| Provider | `local` / `openrouter` / `openai` / `elevenlabs` / `xai` |
| Model / voice | exact ids used |
| Command | full copy-paste command (secrets redacted as `$OPENAI_API_KEY`) |
| Artifacts | output path or stdout format |
| Caveats | experimental tier, unreliable timestamps, long-form remote limits |

## Load order

1. This file (`SKILL.md`) — always for STT/TTS tasks.
2. `references/stt.md` and/or `references/tts.md` for the direction in use.
3. `references/tooling.md` for doctor, cache, cleanup, batch, support.
4. `references/do-not.md` before any remote/credential advice.
5. `references/embed.md` only for library/FFI hosts.
6. In-repo docs if deeper: `docs/guide/providers.md`, `docs/guide/tts.md`,
   `docs/getting-started/cli.md`, `docs/operations/troubleshooting.md`.
