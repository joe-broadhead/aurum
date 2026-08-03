# Provider platform qualification (JOE-1943)

This document is the **release gate** for the local/remote speech provider
platform (epic JOE-1932 / GitHub #59). It defines support tiers, deterministic
CI expectations, protected integration evidence, and demotion/rollback rules.

## Support tiers (JOE-2223)

| Tier | Meaning | Default? |
|------|---------|----------|
| **supported** | Reviewed factory + mocks + **fresh (≤30d) protected inference evidence** | Local only until remote evidence lands |
| **experimental** | Implemented + mocks; evidence missing/stale/limited | No |
| **explicit-only** | Deliberate selection only; hidden from normal recommendations | No |

**Entry rule:** a remote route may not be labelled `supported` without a
versioned evidence record under `evals/provider-evidence/`. Release gate:

```bash
./scripts/check_provider_evidence.sh
```

Missing or stale evidence for a claimed supported remote **fails the release**.
Remediation: restore the route, **demote** in index/docs/changelog, or remove
the claim. No silent cloud fallback.

Code: `aurum_core::provider_platform::evidence` (`SupportTier`,
`ProviderEvidenceRecord`, `evaluate_supported_evidence_gate`).

Current product defaults remain **on-device**:

| Operation | Default provider | Notes |
|-----------|------------------|--------|
| STT | `local` | whisper.cpp; no key |
| TTS | `local` | Kitten ONNX; no key |
| Cleanup | `rules` | Local heuristics |

Remote providers (`openrouter`, `openai`, `elevenlabs`, `xai` / alias `grok`)
are **opt-in** only.

### Provider tiers (JOE-2223 evidence programme)

| Provider | STT | TTS | Tier | Evidence notes |
|----------|-----|-----|------|----------------|
| `local` | yes | yes | **supported** | Offline evidence in `evals/provider-evidence/` |
| `openrouter` | yes | yes | **experimental** | Mocks + catalogue probe; protected smoke pending for promotion |
| `openai` | yes | yes | **experimental** | Mocks; promote only with fresh protected evidence |
| `elevenlabs` | — | yes | **experimental** | Mocks; voice_id explicit; protected smoke pending |
| `xai` | yes | yes | **experimental** | Official `/v1/stt` + `/v1/tts`; mocks; protected smoke pending |

Demote by removing the supported claim + evidence, updating the matrix, product
contracts (`generate_product_contracts --write`), and changelog in one PR.

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

Evidence records are **versioned JSON** (`ProviderEvidenceRecord`, schema 1)
under `evals/provider-evidence/`. Required fields: provider, operation, model,
support tier, protocol contract, executed_at_unix, auth_ok, passed, failure
category, optional latency/byte counts and backend/timestamp honesty.
**Never** retain input text/audio, raw provider bodies, keys, or private voice
IDs (public records use reviewed aliases).

### Drift detection

Compare reviewed model allowlists to vendor discovery with
`detect_catalogue_drift` — discovery **never** auto-expands the trusted
registry. A human review must approve new models/voices/origins.

### Cost / retention guidance

Aurum does not claim vendor zero-retention. Operators must read vendor privacy
policies. Remote selection is deliberate; local remains the default.

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

### OpenRouter privacy prerequisite

OpenRouter account **privacy / data policy / guardrails** must allow the chosen
model, or every request fails with “No endpoints available matching your
guardrail restrictions.” Operators: https://openrouter.ai/settings/privacy

### Catalogue probe (JOE-2213)

Keep reviewed registries aligned with live vendor catalogues so defaults do not
ship dead:

```bash
# Offline (CI-safe): dump static registries + fail if a product default is missing
./scripts/probe_provider_catalogues.sh --offline \
  --out dist/provider-catalogue/PROBE_REPORT.md

# Live (keys in env; list endpoints only — no synthesis audio/text):
OPENROUTER_API_KEY=… OPENAI_API_KEY=… \
  ./scripts/probe_provider_catalogues.sh --live \
  --out dist/provider-catalogue/PROBE_REPORT_LIVE.md
```

GitHub Actions: workflow **Provider catalogue probe** (`workflow_dispatch`;
optional `live=true` with repository secrets).

- **Static FAIL on a default** → demote/replace the constant before release.
- **Live FAIL on a default** → open a demotion PR; do not ship.
- OpenRouter TTS remains **experimental** until protected smoke (JOE-1978).

### Protected inference smoke (JOE-2229)

Catalogue probe proves list membership. **Inference smoke** proves a short
synthetic STT/TTS call still succeeds on the reviewed route.

```bash
# CI-safe: evidence schema + canary only (no network)
./scripts/protected_provider_smoke.sh --dry-run --out dist/provider-smoke

# Operator / protected Actions: short synthetic smokes when secrets are set
OPENAI_API_KEY=… OPENROUTER_API_KEY=… \
  ./scripts/protected_provider_smoke.sh --live --out dist/provider-smoke
```

GitHub Actions: workflow **Provider protected smoke** (`workflow_dispatch`;
`live=true` for credentialed runs). Artifacts are redacted records only —
no audio, transcripts, or keys.

**Promotion is human-gated:** a passing live record does **not** auto-edit
`evals/provider-evidence/index.json` or product surfaces. Copy reviewed JSON
into the evidence pack, mark the route `supported`, regenerate contracts, and
ship demotion surfaces in one PR.

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
