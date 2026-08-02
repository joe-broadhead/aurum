# Provider platform qualification (JOE-1943)

This document is the **release gate** for the local/remote speech provider
platform (epic JOE-1932 / GitHub #59). It defines support tiers, deterministic
CI expectations, protected integration evidence, and demotion/rollback rules.

## Support tiers

| Tier | Meaning | Default? |
|------|---------|----------|
| **supported** | Reviewed models, mock CI coverage, docs, operator path | No (except `local`) |
| **experimental** | Implemented + mocks, limited real evidence | No |
| **explicit-only** | Requires deliberate config; may be model-gated | No |

Current product defaults remain **on-device**:

| Operation | Default provider | Notes |
|-----------|------------------|--------|
| STT | `local` | whisper.cpp; no key |
| TTS | `local` | Kitten ONNX; no key |
| Cleanup | `rules` | Local heuristics |

Remote providers (`openrouter`, `openai`, `elevenlabs`, `xai` / alias `grok`)
are **opt-in** only.

### Provider tiers (2026-08-02)

| Provider | STT | TTS | Tier | Evidence notes |
|----------|-----|-----|------|----------------|
| `local` | yes | yes | **supported** | Full CI; no network |
| `openrouter` | yes | yes | **supported** (reviewed models) | Mocks + prior STT prod path; TTS mock CI |
| `openai` | yes | yes | **supported** (reviewed models) | Mocks; credentialed smoke optional |
| `elevenlabs` | — | yes | **supported** (reviewed models) | Mocks; voice_id explicit |
| `xai` | yes | yes | **experimental** | Mocks; REST only; realtime deferred |

Demote a remote model by removing it from the reviewed registry (fail closed)
without touching local paths.

## Deterministic PR CI (required)

Provider-touching PRs must keep green:

| Check | What it proves |
|-------|----------------|
| Unit/mock HTTP per provider | Auth missing, local_only, success PCM/JSON |
| `check_builtin_conformance` | Descriptor vs capabilities honesty |
| Header/origin isolation | No cross-provider credential headers |
| Secret canary matrix | No key/payload echo on doctor/support/errors |
| STT-only / `--no-default-features` | TTS-gated code does not break STT builds |
| Fuzz smoke (PCM/WAV remote normalize) | Trust boundary parsers |

## Protected integration evidence

Real keys must **not** appear in ordinary PR CI or the repository.

| Vertical | Smoke | Environment |
|----------|-------|-------------|
| OpenRouter TTS | Short phrase → mono PCM | Protected Actions env |
| OpenAI STT/TTS | Short fixture / phrase | Protected Actions env |
| ElevenLabs TTS | Short phrase + real voice_id | Protected Actions env |
| xAI STT/TTS | Optional; else record deferral | Protected Actions env |

Evidence record fields (redacted): provider, model, voice, UTC date, Aurum
commit, latency_ms, encoded/decoded byte counts, result metadata, pass/fail.
**Never** retain input text/audio, raw provider bodies, or keys.

## Engine remote admission (JOE-1975)

Engine-built remote providers hold the engine-local `ResourceGovernor` and
acquire `PermitKind::Remote` for the full operation. HTTP send and body reads
are interruptible via `OpContext` cancel/deadline (not only the long client
timeout). Convenience constructors without a build context still use the
documented process-global governor.

## No-network guarantee

With all of `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ELEVENLABS_API_KEY`, and
`XAI_API_KEY` set to synthetic values:

1. Default CLI/config paths use `provider=local` / `tts_provider=local`.
2. Local STT/TTS perform **zero** HTTP (engine registry + factories).
3. `local_only=true` rejects remote factories before request construction.

Regression: `provider_platform` isolation tests + local unit suite.

## Privacy / data flow (summary)

| Selection | Leaves machine | Recipient |
|-----------|----------------|-----------|
| Default local STT/TTS | No | — |
| `--provider openrouter` STT | Encoded audio | OpenRouter (+ upstream) |
| `--provider openrouter` TTS | Synthesis text | OpenRouter (+ upstream) |
| `--provider openai` | Audio or text | OpenAI first-party |
| `--provider elevenlabs` | Synthesis text | ElevenLabs |
| `--provider xai` / `grok` | Audio or text | xAI |

Account-level retention/logging is **vendor-dependent**; Aurum does not claim
zero-retention on behalf of third parties. See also
[credential hygiene](credential-hygiene.md).

## Release gate checklist

Before publishing a release that includes provider-platform changes:

- [ ] Local-only smoke (no real keys): STT + TTS + doctor
- [ ] Provider matrix doc matches registry (`docs/guide/provider-matrix.md`)
- [ ] Changelog mentions remote opt-in without weakening offline defaults
- [ ] Demotion path known (yank model from reviewed registry)
- [ ] Human review of privacy wording and support tiers
- [ ] Protected remote smokes optional but recorded if support tier is *supported*

## Protocol drift

When a vendor API changes:

1. Update golden/mock fixtures and reviewed model catalogues in the same PR.
2. Keep fail-closed for unknown models.
3. Bump evidence version notes in this file and the provider matrix.

## Related

- ADR-002 provider registry
- [Provider matrix](../guide/provider-matrix.md)
- [Remote audio](../guide/remote-audio.md)
- [Release gate](release-gate.md)
