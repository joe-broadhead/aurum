# Do not (hard rules for agents)

## Product surface

- Do **not** invent CLI flags, config keys, model IDs, or voice IDs.
- Do **not** change the product default STT model (`base`) or TTS pack without an evidence review.
- Do **not** select experimental STT models (e.g. `large-v3-q5_0`) via profiles.
- Do **not** enable remote providers implicitly because a key exists in the environment.
- Do **not** remap local Kitten voices (`Luna`, …) to ElevenLabs/OpenAI/OpenRouter voice ids.
- Do **not** suggest dead OpenRouter OpenAI-family TTS ids (`openai/gpt-4o-mini-tts*`).
- Do **not** claim ElevenLabs has STT in Aurum.
- Do **not** claim remote STT/TTS works on **aurum-ffi** (C ABI is local-only).
- Do **not** claim multi-tenant isolation or built-in microphone capture.
- Do **not** claim a stable `1.0` API — continuous **0.0.x**; pin tags.

## Privacy and security

- Do **not** paste API keys, `.env` contents, audio files, or full transcripts into chat by default.
- Do **not** log secrets in scripts you write for the user; use env vars.
- Do **not** attach unredacted support data; use `aurum support-bundle`.

## Evidence and claims

- Do **not** invent WER, RTF, or “production ready” claims without a retained report.
- Do **not** treat OpenRouter chat multimodal as dedicated ASR for subtitle accuracy.
- Do **not** treat experimental providers (`xai`, OpenRouter TTS) as fully supported tiers.

## Ops

- Do **not** expand FFI ABI casually.
- Do **not** commit generated TTS demo WAVs or model binaries.
- Do **not** push, tag, release, or publish crates without explicit user GO.
