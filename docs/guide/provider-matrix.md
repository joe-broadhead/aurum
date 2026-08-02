# Provider matrix

Snapshot of compiled builtin registry capabilities (JOE-1943). Defaults are
**local** for STT and TTS. Remote rows require deliberate selection and a key.

| Provider | STT | TTS | Auth env | STT models (reviewed) | TTS models (reviewed) | Streaming (Aurum) | Local-only OK |
|----------|-----|-----|----------|----------------------|----------------------|-------------------|---------------|
| `local` | yes | yes | — | whisper catalogue | Kitten / Kokoro packs | no | yes |
| `openrouter` | yes | yes | `OPENROUTER_API_KEY` | reviewed ASR + chat multimodal | `hexgrad/kokoro-82m` (default), `fish-audio/s1`, `sesame/csm-1b`, `minimax/speech-2.8-turbo` (mp3) | no | no |
| `openai` | yes | yes | `OPENAI_API_KEY` | `whisper-1`, `gpt-4o-*-transcribe` | `tts-1`, `tts-1-hd`, `gpt-4o-mini-tts` | no | no |
| `elevenlabs` | — | yes | `ELEVENLABS_API_KEY` | — | `eleven_multilingual_v2`, turbo/flash v2.5 | no | no |
| `xai` (`grok` alias) | yes | yes | `XAI_API_KEY` | `xai-stt` (POST `/v1/stt`) | `xai-tts` voices `eve|ara|leo|rex|sal` (POST `/v1/tts`) | experimental | no (REST only; no realtime) |

### Formats and honesty

| Provider | STT timestamps | TTS wire prefer | Result `backend_kind` |
|----------|----------------|-----------------|------------------------|
| `local` | yes (ASR) | native PCM | `asr` / TTS `local` |
| `openrouter` STT | path-dependent | — | `asr` or `llm_assisted` |
| `openrouter` TTS | — | PCM | TTS `remote` |
| `openai` STT | whisper-1 yes; gpt-4o-* no | — | `asr` |
| `openai` TTS | — | PCM | `remote` |
| `elevenlabs` | — | `pcm_24000` | `remote` |
| `xai` | model-dependent | PCM | STT `asr` / TTS `remote` |

### Discovery

Provider listing via `ProviderRegistry::builtin()` / engine `registry()` is
**credential-free** (static descriptors). Live voice-list network refresh is
not required for basic operation.

FFI: remote execution is **not** exposed on the C ABI until a separate design;
capability claims must not over-advertise FFI remote support.

Update process: when adding a provider/model, edit factories + this matrix +
`docs/operations/provider-qualification.md` in the same PR.
