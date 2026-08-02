# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.19] - 2026-08-02

v0.0.19 is the **post-provider security freeze**: provider platform (JOE-1932
/ #59) plus remediations JOE-1975–1980, F-005 verified-snap staging (JOE-1918),
F-006 residual honesty (JOE-1919), and freeze checklist for JOE-1981 → JOE-1920
qualified human retest. Pre-provider `v0.0.18` remains historical only.

### Fixed

- **JOE-1918 (F-005 re-open):** digested TTS pack/catalogue artifacts stage into
  process-owned `tts/verified-snaps/<sha256>/` before ORT/NPZ open so pack-path
  swaps after verify cannot change native load bytes.
- **JOE-1919 / JOE-1981:** formal residual + post-provider freeze checklist docs
  (`external-review-disposition-2026-08-02.md`, `post-provider-security-freeze.md`).
- **JOE-1978:** OpenRouter TTS default is exact dated id
  `openai/gpt-4o-mini-tts-2025-12-15`; undated bare id removed; registry records carry
  tier/evidence metadata; TTS remains **experimental** until protected smoke.
- **JOE-1979:** `ProviderId` Serde goes through `parse`; OpenRouter STT factory
  capabilities are Auto-route-aware; ElevenLabs `voice_id` is path-segment encoded;
  remote TTS rejects local `pack_dir`/`allow_unverified`.
- **JOE-1980:** remote providers retain `SecretString` (not plaintext `String`);
  network errors use `public_network_reason` (no URL/path echo from reqwest Display).
- **JOE-1976:** xAI STT/TTS use official REST `POST /v1/stt` and `POST /v1/tts`
  (not OpenAI `/audio/*`). Product ids `xai-stt` / `xai-tts`; voices
  `eve|ara|leo|rex|sal`. Stability **Experimental** end-to-end until protected
  smokes. Legacy `grok-asr*` / `grok-tts*` / OpenAI voice ids fail closed.
- **JOE-1977:** remote TTS fails closed on MIME/format mismatch via
  `resolve_encoded_format` — missing/generic/JSON/HTML Content-Type never
  defaults to PCM; rate/channel params must match the requested wire format.
- **JOE-1975 (High):** engine-bound remote STT/TTS acquire `PermitKind::Remote` from
  the **engine-local** governor (not `process_global`), and HTTP `.send()` / body
  reads race cancel + absolute deadline via `send_with_op` /
  `read_body_limited_with_op`. Independent engines isolate remote admission;
  short op deadlines interrupt before the long client timeout.

### Changed

- **OpenRouter app attribution:** all OpenRouter HTTP calls identify as **Aurum**
  via `HTTP-Referer` (`https://github.com/joe-broadhead/aurum`), preferred
  `X-OpenRouter-Title` (plus legacy `X-Title`), and `X-OpenRouter-Categories`
  (`audio-gen,cli-agent`). Constants are exported from `aurum_core::remote`.

### Added

- **JOE-1943:** provider platform qualification — support tiers, release-gate
  checklist, provider matrix, privacy/data-flow summary, cross-provider isolation
  tests, long-lived `engine_providers` example; FFI docs restate remote
  unsupported on C ABI.
- **JOE-1942:** xAI REST STT/TTS (`xai`, config alias `grok`) via `XAI_API_KEY` and
  `XaiHttpPolicy`; OpenAI-compatible speech request shape; reviewed `grok-asr*` /
  `grok-tts*` catalogues; realtime streaming not implemented (fail closed).
- **JOE-1941:** ElevenLabs production TTS (`elevenlabs`) via `ELEVENLABS_API_KEY` /
  `xi-api-key`, reviewed models, explicit `voice_id` (no local alias remap),
  `pcm_24000` + shared normalize.
- **JOE-1940:** first-party OpenAI STT (`whisper-1` / gpt-4o-*-transcribe) and
  TTS (`tts-1` / `tts-1-hd` / `gpt-4o-mini-tts`) via `OPENAI_API_KEY` and
  `OpenAiHttpPolicy` (no OpenRouter headers). Reviewed model registries; PCM
  speech normalize via JOE-1937.
- **JOE-1939:** OpenAI-compatible speech protocol (`OpenAiSpeechRequest`) and
  OpenRouter remote TTS (`/audio/speech`, reviewed model registry, PCM normalize
  via JOE-1937). `openrouter` registers for STT **and** TTS factories.
- **JOE-1938:** `AurumEngine` owns the provider registry and resolves STT/TTS via
  `stt_provider` / `tts_provider` (scoped secrets, engine pools/governor/metrics).
  CLI `aurum` / `aurum tts` / `aurum batch` use the same path (no per-vendor
  `match` growth). Defaults remain local; remote still requires deliberate
  selection + key.
- **JOE-1937:** bounded remote-audio normalization (`normalize_remote_audio`) for
  remote TTS wire formats (PCM s16le / WAV in-process, MP3 via supervised FFmpeg)
  producing mono `i16` PCM under encoded/decoded caps; TTS `BackendKind::Remote`
  and honesty JSON (`local` \| `remote`); guide
  [remote-audio.md](docs/guide/remote-audio.md).

## [0.0.18] - 2026-08-01

v0.0.18 is the **external-review security remediation freeze**: High F-001/F-002
fixed and retested, Medium F-003–F-006 code remediations, crates.io credential
operator rotation complete, and a frozen post-fix candidate for JOE-1920 human
sign-off.

### Security

- **F-001 (JOE-1914):** type-safe credential non-disclosure; closed provider-code
  allowlist; canary matrix across doctor/support/errors (PRs #52, #58).
- **F-002 (JOE-1915):** immutable tag-only crates.io publish; fail-closed
  `--no-verify` policy checker in CI/release_gate/publish (PRs #53, #58).
- **F-003 (JOE-1916):** doctor exclusive probe + support-bundle OutputTransaction
  (PR #54).
- **F-004 (JOE-1917):** ResourceGovernor construction validation + checked CPU
  accounting (PR #55).
- **F-005 (JOE-1918):** artifact/BYOM TOCTOU and durable download path (PR #56).
- **F-006 (JOE-1919):** full-SHA provenance identity in generate/verify (PR #57).
- **JOE-1715:** crates.io credential rotated; env-only `CARGO_REGISTRY_TOKEN`;
  `crates-token-check` workflow for safe positive auth (PRs #60, #61).

### Documentation

- Formal Medium residual disposition table for F-003–F-006
  (`docs/operations/external-review-disposition-2026-08-01.md`).

## [0.0.17] - 2026-08-01

v0.0.17 is **Group L — RC exit + external review pack**: exit report generator,
reviewer brief, downstream consumer gate, and frozen native inventory.

### Added

- **JOE-1904:** `scripts/generate_rc_exit_report.sh` + `docs/operations/rc-exit.md`
  (human sign-off left blank).
- **JOE-1901:** `docs/operations/external-review-brief.md` + findings disposition
  template for independent review.
- **JOE-1903:** `scripts/rc_downstream_check.sh` (minimal Rust consumer + C/C++
  FFI examples) + CI `rc-exit-pack`.
- **JOE-1902:** `native-components.md` includes locked whisper/ort/toolchain
  versions from cargo metadata; freeze check requires it.

## [0.0.16] - 2026-08-01

v0.0.16 is **Group K — RC freeze foundations**: compatibility freeze inventory,
dogfood checklist automation, rollback rehearsal, and support/security-fix
policy for the path to 1.0 RC.

### Added

- **JOE-1896:** `docs/operations/rc-freeze.md` freeze inventory +
  `scripts/rc_freeze_check.sh` + CI job `rc-freeze-check`.
- **JOE-1897:** `docs/operations/rc-dogfood.md` multi-platform checklist +
  `scripts/rc_dogfood_checklist.sh` + CI `rc-dogfood-smoke` evidence artifacts.
- **JOE-1895:** `docs/operations/rc-rollback.md` + `scripts/rehearse_rc_rollback.sh`
  (supersede-not-rewrite, human sign-off block).
- **JOE-1898:** `docs/operations/support-policy.md` support / security-fix /
  deprecation / incident / first-patch readiness.

## [0.0.15] - 2026-08-01

v0.0.15 is **Group J — security evidence slice**: threat-model control matrix,
independent release verify, model pin revocation rehearsal, and disclosure
tabletop evidence pack.

### Added

- **JOE-1893:** Versioned threat-model control matrix with High/Critical threat
  → control → evidence → residual dispositions (`docs/operations/threat-model.md`).
- **JOE-1891:** Independent download+cosign verify
  (`scripts/independent_release_verify.sh`, `.github/workflows/release-verify.yml`)
  including negative checksum path.
- **JOE-1892:** Model-manifest revocation runbook + dry-run rehearsal script.
- **JOE-1890:** Confidential disclosure tabletop + intake checklist evidence pack.

## [0.0.14] - 2026-08-01

v0.0.14 is **Group I — third 1.0 QE depth slice**: Miri pure suite, sanitizer
+ concurrency stress, trust-boundary coverage reports, and scoped mutation
testing.

### Added

- **JOE-1889:** `scripts/run_miri.sh` curated pure-Rust Miri filters + CI job
  `miri`; gaps (Tokio/whisper/ORT) documented in `docs/operations/qe-depth.md`.
- **JOE-1887:** `scripts/run_sanitizers.sh` Linux ASan pure filters + FFI
  concurrency stress tests; CI job `sanitizer-stress`; platform gaps documented.
- **JOE-1888:** `scripts/coverage_trust.sh` module-scoped coverage report
  (`TRUST_COVERAGE.md`) with soft floors; CI artifact upload.
- **JOE-1886:** `scripts/run_mutants.sh` cargo-mutants smoke (sharded) over
  domain/dto/error/cleanup/providers; survivor policy in qe-depth.md.

## [0.0.13] - 2026-08-01

v0.0.13 is **Group H — second 1.0 assurance slice**: forced cosign keyless
release attestation, expanded fuzz + scheduled campaigns, Tier A clean-install
CI matrix, and two-builder reproducibility reports.

### Added

- **JOE-1882:** `release.yml` always produces `SHA256SUMS.bundle` via cosign
  keyless (OIDC); `verify_release_assets.sh` supports `AURUM_REQUIRE_COSIGN=1`
  with documented identity / rotation / revocation in
  `docs/operations/provenance.md`.
- **JOE-1884:** Fuzz targets `wav_parse` + `ffi_validators`; public
  `try_load_wav_file`; scheduled `fuzz-campaign.yml` with crash artifact
  upload; fuzzing ops doc triage section.
- **JOE-1883:** `ci.yml` `clean-install` matrix on ubuntu-24.04 / macos-14 /
  windows-latest (`--from-source`); platform-support docs link the job.
- **JOE-1885:** `scripts/compare_release_builds.sh` + `docs/operations/reproducibility.md`;
  CI repro-smoke and release `repro-compare` variance report artifacts.

## [0.0.12] - 2026-08-01

v0.0.12 is **Group G — first 1.0 assurance slice**: formal SBOM, structured
provenance verification, cargo-fuzz smoke, and platform support docs.

### Added

- **JOE-1859:** Formal CycloneDX 1.5 (`aurum.cdx.json`) + SPDX 2.3 lite
  (`aurum.spdx.json`) SBOMs from `scripts/generate_sbom.sh`; release verify
  requires them.
- **JOE-1860:** Structured `PROVENANCE.json` generation and verification;
  optional cosign keyless path documented in `docs/operations/provenance.md`.
- **JOE-1861:** `fuzz/` cargo-fuzz targets (config, DTO, segment, cleanup,
  output) + CI short smoke on nightly; ops guide `docs/operations/fuzzing.md`.
- **JOE-1863:** Platform support matrix + `scripts/clean_install_smoke.sh`
  clean-install qualification.

## [0.0.11] - 2026-08-01

v0.0.11 is **Group F — SDK domain privacy residual and FFI engine pools**:
private `AudioInput`/`TranscriptionResult`, validated DTO conversion, and
FFI engine-local STT/TTS residency.

### Added

- **JOE-1809:** Private `AudioInput` / `TranscriptionResult` fields with accessors;
  `TranscriptionResult::try_from_dto` / `SttResultDto::into_domain` for validated
  DTO → domain conversion (cannot skip segment/duration validation).
- **JOE-1810:** FFI `Engine` owns local STT/TTS pools + governor; jobs and
  exclusive STT share engine pools; `shutdown_engine` clears engine residency
  without touching process-global residual.

## [0.0.10] - 2026-08-01

v0.0.10 is **Group E — provider routing and remote operation maturity**:
capability-authoritative OpenRouter auto routing, end-to-end OpContext stages,
uniform TTS soft-deadline for packs, and transactional batched remote cleanup.

### Added

- **JOE-1829:** Capability-authoritative OpenRouter `auto` STT routing via a
  reviewed static registry (`OPENROUTER_STT_REGISTRY`). Unknown models fail
  closed; explicit `chat` / `transcriptions` modes still accept any model id.
- **JOE-1831:** End-to-end `OpContext` stages on remote STT and cleanup; resource
  governor permit wait uses `Condvar` notify instead of busy sleep polling.
- **JOE-1830:** Uniform TTS soft-deadline contract for built-in and local-pack
  paths (caller timeout vs still-running native work that retains permits).
- **JOE-1832:** Streaming file→base64 for chat-audio STT; transactional batched
  remote per-segment cleanup with stable segment ids (commit only after all
  batches succeed).

## [0.0.9] - 2026-08-01

v0.0.9 is **Group D — domain privacy, doctor ops, ABI, and CLI engine pools**:
private `Segment` fields with domain primitives, doctor offline/writable checks
and redaction tests, expanded FFI ABI size snapshots, and CLI/batch local paths
on `AurumEngine` model pools.

### Added

- **JOE-1786:** `Segment` fields private with accessors; domain primitives
  `SampleRateHz` / `FiniteDurationSecs` / `ModelId`;
  `Segment::from_parts_unchecked` for trusted paths.
- **JOE-1783:** doctor `cache_writable` + explicit offline check; redaction
  tests for doctor JSON and support bundles.
- **JOE-1785:** expanded ABI size snapshot tests; FFI install docs for C11/C++17
  examples.
- **JOE-1795:** CLI local STT/TTS and batch local path use `AurumEngine` pools.

## [0.0.8] - 2026-08-01

v0.0.8 is **Group C — engine ownership completion**: per-engine whisper/TTS model
pools, high-level `AurumEngine` STT/TTS entry points, and progressive
`AudioInput` domain hardening. Process-global pools remain for non-engine
`Provider::new` paths (CLI default).

### Added

- **JOE-1784:** engine-owned `SttContextPool` / `TtsSessionPool`;
  `LocalWhisperProvider::with_runtime` / `LocalTtsProvider::with_runtime`;
  independent engines no longer share model residency; `clear_model_caches` /
  shutdown clear engine pools only.
- **JOE-1787:** `AurumEngine::transcribe` / `transcribe_pcm` / `preload_stt` /
  `synthesize` (feature `tts`) with engine-local governor + metrics.
- **JOE-1786 (progressive):** `AudioInput::from_pcm*` rejects non-finite
  samples and invalid duration; domain docs updated.

## [0.0.7] - 2026-08-01

v0.0.7 is the **SDK hardening residual** after owned-engine foundations v0.0.6:
`SecretString` on the OpenRouter API key field, fail-closed CLI/batch
`ValidatedConfig` re-validation after overrides, and fail-closed
`TranscriptionResult::try_local` / `try_openrouter` builders.

### Changed

- **JOE-1779 residual:** `Config.openrouter_api_key` is `Option<SecretString>`;
  CLI/batch re-validate via `ValidatedConfig` after CLI overrides.
- **JOE-1781 residual:** `TranscriptionResult::try_local` / `try_openrouter`
  fail closed on invalid segments/duration.

## [0.0.6] - 2026-07-31

v0.0.6 is the **owned SDK foundations** release after evidence residual v0.0.5:
`ValidatedConfig`, `SecretString`, and an owned `AurumEngine` for library hosts,
with safer segment construction and honest process-global model-cache residual
docs. Group B / JOE-1654 is partially complete — per-engine model isolation and
richer engine STT/TTS APIs remain follow-ups.

### Added

- **JOE-1654 / JOE-1779 / JOE-1782:** `ValidatedConfig`, `SecretString`, and
  owned `AurumEngine` (engine-local governor + metrics; process-global model
  cache residual documented). CLI `doctor` / `support-bundle` use the engine.
- **JOE-1781:** `Segment::try_new` / `validate` reject NaN/Inf/inverted timings.
- **JOE-1780:** compatibility classification and `docs/library/engine.md`.

## [0.0.5] - 2026-07-31

v0.0.5 is the **evidence residual** release after product-proof v0.0.4: a public
offline STT evaluation corpus with retained model matrix reports, TTS objective
and pilot listening evidence, and named-hardware performance baselines on Apple
Silicon Metal.

### Added

- **JOE-1731:** public offline STT corpus (`evals/corpus.public-v1.json`) with
  synthetic multi-accent speech, noise mix, silence/non-speech fixtures,
  checksums, and `scripts/run_stt_eval_matrix.py` plus retained matrix reports.
- **JOE-1735:** Kitten/Kokoro objective PCM report + listening pilot round 001
  (explicitly not MOS) under `evals/reports/listening/`.
- **JOE-1739:** named-hardware Apple Silicon Metal STT/TTS wall-time baselines
  under `evals/reports/perf/`; evidence summary in `docs/operations/evidence-v004.md`.
- **JOE-1715 (docs):** credential rotation runbook for post-publish token revoke.

## [0.0.4] - 2026-07-31

v0.0.4 is the **product proof** patch after v0.0.3: adoption polish, reproducible
evidence foundations, agent skills, and fail-closed release hygiene. Versions
continue as **0.0.x** until a deliberate major step.

### Security

- **JOE-1715:** CI/release install of `cargo-audit` / `cargo-deny` is **pinned and
  fail-closed** (no `|| cargo install latest` fallback); `scripts/check_security_tool_pins.sh`
  enforces workflow policy; credential rotation runbook in
  `docs/operations/credential-hygiene.md` (operator must still revoke any
  chat-disclosed crates.io token).

### Added

- **JOE-1726:** `aurum batch` — bounded, resumable multi-file transcription with
  versioned `aurum-batch-manifest.json`, deterministic naming, dry-run, resume,
  and retry-failed.
- **JOE-1720:** first-class release binary installer (`scripts/install.sh
  --from-release` with SHA256 verify, upgrade, uninstall preserving cache/config);
  `aurum completions <shell>` and `aurum man`.
- **JOE-1728:** `aurum support-bundle` privacy-safe offline diagnostics; early-adopter
  issue template; redacted path tokenisation.
- **JOE-1723:** opt-in quality profiles (`speed` / `balance` / `quality`) and
  `aurum models recommend --profile …`; explicit `--model` wins; **default remains
  `base`**; experimental models never selected by profiles.
- **JOE-1778:** root `skills/` agent packs (`aurum-cli`, `aurum-batch`, `aurum-embed`,
  `aurum-support`) following dbt-nova package shape.
- **JOE-1731 / 1735 / 1739 foundations:** expanded smoke corpus v2, synthetic CC0
  audio generators, silence FP + repetition metrics, TTS listening scorecard,
  named-hardware performance report script/docs.

### Fixed

- **JOE-1717:** documentation truth — architecture/AGENTS no longer claim FFI is
  STT-only; generated CLI help snapshot under `docs/reference/cli-help.md`.

### Notes

- Quality/performance **product claims** still require operator runs against
  licensed speech and retained hardware reports; smoke fixtures are not speech
  accuracy proof.
- Profile evidence version: `0.0.4-provisional-smoke`.

## [0.0.3] - 2026-07-31

v0.0.3 is a **correctness and safety hotpatch** over the epic-per-PR batch that
shipped foundations under `0.0.2`. It closes the Track A audit blockers for
output durability, model identity, residency, FFI lifetime, FFmpeg supervision,
BYOM containment, and honest model disposition. Formal 1.0 assurance (external
review, continuous fuzzing, Sigstore/SLSA, multi-RC freeze) remains post-release
backlog (JOE-1652–1655).

### Fixed

- **JOE-1647/1648 fourth-pass residuals** (post PR #35 re-audit):
  - `aurum_job_take_*`, `aurum_job_poll`, `aurum_transcript_segment`, and
    `aurum_capabilities` zero/null outputs **before** argument validation
  - FFmpeg **decode** races cancellation against pipe drains so stalled reads
    cannot delay cancel until the wall-clock timeout; kill+reap on cancel
- **JOE-1646/1647/1648 third-pass residuals** (post PR #34 re-audit):
  - Registry `clear` / TTS `clear_sessions` only drop **idle** entries; active
    pins keep residency accounting so same-key reload cannot multiply sessions
  - Fallible C out-pointers for create/job-start/cleanup are nulled before
    export admission; take/poll/segment/capabilities completed in fourth-pass
  - Upload encode propagates cancel / deadline / size-cap errors (no WAV success
    fallback for control-plane failures); OpenRouter passes cancel into encode
- **JOE-1646/1647/1648 re-audit** (post PR #33 re-review):
  - STT/TTS registry **atomic pin** for full operation lifetime (`get_and_pin` /
    `insert_and_pin`); TTS holds pin inside the synth worker (active clear is
    pin-aware — see third-pass residual above)
  - Panic-safe singleflight `LeaderGuard` (abandoned loads unblock waiters)
  - FFI `export_depth` spans full C call including last_error; destroy frees
    only on successful drain; C11/C++17 examples run in CI on Linux/macOS
    (Windows MSVC host-link of examples deferred)
  - FFmpeg encode drains stderr **concurrently** with child poll; explicit kill/reap;
    cancel/deadline are not swallowed by WAV fallback (third-pass)

- **JOE-1643 / JOE-1644–1650** v0.0.3 audit remediation:
  - **JOE-1644** Output `NoClobber` uses Unix hard-link publish (race-safe vs
    rename-overwrite); file and parent-dir sync errors propagate; temp files
    created with owner-only mode at create time (unique PID+nanos suffix;
    exclusive `create_new` is the collision gate)
  - **JOE-1645** Every trusted STT catalogue model has reviewed SHA-256 + exact
    size; unpinned downloads refuse to publish; sidecar never authenticates
  - **JOE-1646** STT/TTS residency: registry is sole long-lived owner; singleflight
    only coalesces in-progress loads; TTS sessions share a process-global budget
  - **JOE-1647** FFI engine close waits for exclusive blocking ops; `aurum_engine_close`
    status API; destroy frees only after successful drain; closed engines reject
    new work; complete fallible out-pointer init
  - **JOE-1648** Decode and upload FFmpeg use supervised lifecycle (`-nostdin`,
    concurrent drain, deadline/cancel, kill/reap; upload has no `-y` clobber)
  - **JOE-1649** BYOM pack artifacts reject symlinks/path escapes; manifests use
    secure output transactions
  - **JOE-1650** `large-v3-q5_0` marked experimental; repetition degeneration
    detector in the normalization **report** API (not ordinary CLI/JSON emit);
    model guidance from dogfood (not formal WER)

### Added

- **JOE-1618** Curated Kokoro-82M TTS adapter (opt-in, not default):
  - Catalogue model `kokoro-82m-int8` with immutable SHA-256 pins for int8 ONNX,
    voices pack, and config vocab (Apache-2.0 weights)
  - Adapter `kokoro-onnx-v0` enabled for synthesis (tokens/style/speed @ 24 kHz)
  - English voice catalogue (US/GB friendly names mapped to pack internal keys)
  - Kokoro phoneme vocab + misaki-rs G2P (MIT; no GPL espeak)
  - NPZ loader accepts Kokoro style shape `(rows, 1, dim)`
  - Docs/ADR updated; Kitten remains the default model

- **JOE-1578** Quality engineering, supply chain and v1.0 release gates:
  - `deny.toml` + CI `cargo audit` / `cargo deny check` security job (JOE-1630)
  - Continuous adversarial parser suite (`adversarial_parsers`) for config, JSON
    formatting, remote bounds, PCM, and TTS manifests (JOE-1631)
  - Deterministic fault / fail-closed I/O suite (`fault_injection`) for output
    transactions, cleanup, normalization, and oversized config (JOE-1632)
  - STT-only (`--no-default-features`) CI job + adversarial tests on every PR
    (JOE-1633)
  - Third-party GitHub Actions pinned to full commit SHAs; `check_action_pins.sh`
    policy gate; fail-closed tag checkout on release (JOE-1634)
  - SBOM inventory script (`generate_sbom.sh`), `PROVENANCE.txt`, SHA256SUMS
    verification (`verify_release_assets.sh`) attached to GitHub Releases
    (JOE-1635)
  - Supported platform tiers + release/reproducibility docs (JOE-1636)
  - Threat model, deployment profiles, hardening baseline (JOE-1637)
  - Disclosure rehearsal procedure in SECURITY.md (JOE-1638)
  - Operator/integrator handbook map (JOE-1639)
  - `scripts/release_gate.sh` fail-closed pre-tag gate toward 1.0 (JOE-1640)

- **JOE-1577** Native SDK, FFI v2 and operability:
  - Explicit engine ownership with per-engine job drain
    (`aurum_engine_shutdown`) that does not poison other engines (JOE-1622)
  - Asynchronous job API: start/poll/wait/cancel/take/free for preload, STT,
    cleanup, and local TTS (JOE-1623/1629)
  - ABI v2: expanded status codes, `aurum_capabilities`, versioned snapshots,
    pointer+length TTS text (JOE-1624)
  - Documented ownership graph and thread-safety model (JOE-1625)
  - C ABI smoke + layout tests for capabilities/jobs (JOE-1626)
  - Process metrics + redacted diagnostic bundle (JOE-1627)
  - `aurum doctor` / `aurum_doctor_json` read-only diagnostics (JOE-1628)

- **JOE-1576** TTS model platform and safe BYOM:
  - Versioned adapter contract + `aurum-tts-manifest.json` pack schema
    (`kitten-onnx-v1`, `fake-sine-v1` conformance fixture, `kokoro-onnx-v0`
    scaffold) — no bare ONNX loading (JOE-1615)
  - Trust modes: `builtin` | `verified` | `local_unverified` with digest/size
    verification and isolated local-pack cache identity (JOE-1619)
  - Adapter/model-pack conformance suite (fast fixtures every PR) (JOE-1616)
  - Kokoro feasibility ADR + scaffold adapter (not product-shipped) (JOE-1617)
  - Validated `[[tts.custom_models]]` catalogue entries (JOE-1620)
  - CLI: `aurum tts adapters|inspect|verify|add` and synth `--pack-dir` /
    `--allow-unverified` (JOE-1621)
  - Honesty JSON / `TtsMetaDto` optional `adapter`, `trust`, `provenance`

- **JOE-1575** Public API contracts and provider architecture:
  - Validated config path (`validate`, `load_from_required`, redacted
    `effective_diagnostic` with source attribution) (JOE-1608)
  - `NormalizationReport` + validated segment helpers (JOE-1609)
  - Language-aware rules cleanup; precompiled regexes; no ambiguous contractions;
    paragraph preservation (JOE-1610)
  - `AurumError` alias, stable `ErrorCategory`, retryability, refined exit codes
    (JOE-1611)
  - Compatibility / deprecation boundary docs (JOE-1612)
  - Provider capability contracts + preflight routing (JOE-1613)
  - Versioned JSON DTOs (`SttResultDto`, `TtsMetaDto`, `ErrorDto`); TTS result
    no longer deserializable as a full domain object with PCM (JOE-1614)

- **JOE-1574** Performance, streaming, and measurable quality:
  - Fixed-capacity PCM ring buffer with NaN/Inf rejection and f64 RMS (JOE-1601)
  - Streaming FFmpeg s16le→f32 and direct WAV i16→f32 without dual full buffers (JOE-1602)
  - Dedicated OpenRouter multipart streams from disk (no base64); chat path drops raw
    before JSON (JOE-1603)
  - Streaming TXT/SRT/JSON writers with output byte budget into secure transactions
    (JOE-1604)
  - `PartialSession` v2: VAD/energy, stable/unstable text, revisions, inflight cap (JOE-1605)
  - Benchmark schema + PR-safe smoke micro-benches and budgets (JOE-1606)
  - Versioned smoke eval corpus with WER/CER scoring and machine-readable reports
    (JOE-1607)

- **JOE-1573** Runtime lifecycle, concurrency, and resource governance:
  - Synchronized FFI lifecycle Running → ShuttingDown → Stopped; `aurum_shutdown_ex`
    returns `AURUM_ERR_BUSY` and does not clear contexts while work is active (JOE-1594)
  - Per-operation `OpContext` (request id, fresh cancel token, absolute deadline) (JOE-1595)
  - Process `ResourceGovernor` with separate permits for model load / STT / TTS / remote /
    blocking work, CPU thread budget, and soft memory reservations (JOE-1596)
  - Singleflight STT context and TTS session loading (JOE-1597)
  - Weighted model residency registry with idle eviction (JOE-1598)
  - STT `n_threads` allocated from the global CPU budget; bounded `spawn_blocking` (JOE-1599)
  - TTS bounded by governor permits; soft deadline keeps native work tracked until return (JOE-1600)
  - New status codes: `AURUM_ERR_BUSY`, `AURUM_ERR_DEADLINE`, `AURUM_ERR_OVERLOAD`
  - Typed provider errors: `DeadlineExceeded`, `Overload`

- **JOE-1572** Safe external I/O and artifact trust:
  - Supervised FFmpeg decode (concurrent stdout/stderr drain, `-nostdin`, protocol
    whitelist, wall-clock deadline, kill+reap)
  - Shared hardened remote HTTP client with OpenRouter-origin credential policy,
    no redirects, optional custom endpoint opt-in (STT + cleanup)
  - Dual OpenRouter STT paths: dedicated `/audio/transcriptions` and multimodal chat
    (`--openrouter-stt-mode auto|chat|transcriptions`)
  - Bounded remote response bodies and transcript/segment validation
  - Structured remote cleanup contract with expansion caps; segment batch helper
  - STT reviewed exact sizes + SHA-256 pins for default/trial models; verify-before-publish
    downloads (no redirects); shared `download` size-cap helpers
  - `aurum cache status|verify|repair` (verify quarantines bad STT artifacts)
  - Hardened NPZ/NPY/config parsers (ZIP/dimension/config byte caps)

### Fixed

- **JOE-1571** Critical correctness and transactional behavior:
  - Long local TTS input is split at sentence/word boundaries using the model’s
    phoneme-token capacity, then synthesized into one WAV (GitHub #15). Policy is
    **complete-or-error** — no silent character truncation.
  - TTS sample-rate metadata always matches adapter-native PCM; non-native
    `sample_rate_hz` overrides are rejected (no metadata-only relabeling).
  - Fixed `TAIL_TRIM = 2000` replaced with signal-aware trailing-silence trim that
    never empties short valid utterances.
  - Voices are validated as model-scoped; JSON/CLI report canonical model/voice IDs.
  - Cleanup applies transactionally: failures leave `TranscriptionResult` unchanged;
    raw vs rendered text/segments are explicit in JSON.
  - STT, cleanup, and TTS file outputs share one secure output transaction
    (exclusive temp, flush/sync, atomic publish, symlink reject, no-clobber/replace).

### Changed

- `prepare_text` rejects oversized TTS input instead of truncating (raise
  `[tts].max_chars` if you intentionally accept longer text; chunking still
  enforces model phoneme limits).
- `apply_cleanup_with_segments` returns `(CleanupResult, CleanupReport)`.
- TTS `--emit-json` includes `chunk_count` and `synthesized_chars`; `text_truncated`
  is always `false` under complete-or-error.

## [0.0.2] - 2026-07-26

### Changed

- **Tagline:** *Speech both ways. On-device by default.* (STT + TTS product honesty)
- Docs and crate metadata refreshed for speech I/O (CLI, core, FFI, site, GitHub description)
- `aurum-core` crate docs describe STT + TTS; STT-only via `default-features = false`

### Fixed

- Stale version pins in docs (`0.0.0` / `v0.0.1` → current)
- Crate keyword limits already enforced for crates.io

## [0.0.1] - 2026-07-26

### Notes

- GitHub Release binaries: **macos-arm64**, **linux-x86_64**, **windows-x86_64** (Intel Mac: build from source — `ort` has no x86_64-apple-darwin cross prebuilts)

### Added

- **TTS MVP:** `aurum tts` local-default text→mono WAV (ONNX KittenTTS nano int8 + MIT G2P)
  - Nested `aurum tts models` / `aurum tts voices` catalogue + cache status
  - Config `[tts]` + `AURUM_TTS_MODEL` / `AURUM_TTS_VOICE` / `AURUM_TTS_LANGUAGE`
  - Pinned SHA-256 voice pack download under `…/aurum/tts/`, atomic WAV write, `--emit-json` honesty schema
  - `aurum-core` module `tts/` behind cargo feature `tts` (default on for CLI; ORT increases binary size)
  - `LocalTtsProvider::clear_sessions` drops loaded ONNX graphs; wall-clock TTS timeout is best-effort
  - `scripts/generate_tts_demos.sh` — regenerate per-voice demo WAVs locally (not committed)
  - TTS full synth integration test is `#[ignore]` (network/cache); empty-text unit always runs
- **`aurum-ffi`** crate: C ABI façade (`include/aurum.h`) for embedders — PCM transcribe, preload, cancel, rules cleanup
- Docs: TTS guide (`docs/guide/tts.md`); native embeds guide (`docs/library/ffi.md`); workspace/architecture updated for three crates + TTS

### Changed

- CLI crates.io package remains **`aurum-stt`** (binary `aurum`); crates and binaries now include local TTS

## [0.0.0] - 2026-07-24

### Added

- Initial experimental release of **Aurum**
- Tagline: *Audio in. Text out. On-device by default.*
- Workspace: `aurum-core` (library) + `aurum` (CLI)
- Local transcription via whisper.cpp (`whisper-rs`, Metal on macOS)
- Process-level `WhisperContext` cache with pre-exit clear (Metal-safe)
- OpenRouter remote provider (LLM-assisted multimodal chat audio); `top_p=1` with `temperature=0`
- Quantized local models (`tiny-q5_1`, `base-q5_1`, turbo/large q5, …)
- `aurum models` — list models and cache status
- Cross-process model download lock (`std::fs::File::lock`)
- JSON: `backend_kind`, `timestamps_reliable`, `cleanup_style`, optional `cleanup_provider` + `original_text`
- OpenRouter SRT refused unless `--allow-unreliable-timestamps`
- Automatic ggml model download + cache; pinned SHA-256 for common models; magic fail-closed
- Config file + `OPENROUTER_API_KEY`; `[cleanup]` defaults
- Actionable error taxonomy (user / environment / provider)
- Output: txt / srt / json
- Post-process: strip special tokens, clamp timestamps, NaN guard
- Audio safety limits during decode; download size cap
- PCM-first embedder API: `from_pcm`, `PcmBuffer`, `transcribe_pcm`, `preload`, `local_only`
- `PartialWindowPolicy` / `PartialClock` for host-driven interim decode
- `CancelFlag` + whisper abort callback
- Cleanup/flow stage: `RulesCleanup` (on-device) + `OpenRouterCleanup` (LLM)
- CLI: `--cleanup`, `--cleanup-provider`, `--cleanup-model`, `--cleanup-segments`
- `aurum cleanup` / `aurum flow` subcommand (stdin or text file)
- Synthetic audio fixtures + `scripts/generate_fixtures.sh`
- MkDocs Material documentation site (house theme)
- CI (multi-OS, MSRV 1.89, docs strict, version sync, integration)
- Release workflows (prepare / tag / multi-platform binaries + SHA256SUMS)
- Community files: CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, SUPPORT, AGENTS
- `scripts/publish_dry_run.sh` for crates.io readiness

### Notes

- `aurum-core` API is experimental until `0.1.0`
- ffmpeg is a required system dependency (not bundled)
- OpenRouter path is LLM-assisted, not dedicated ASR
- crates.io publish is **not** part of automated release yet
- No GitHub Release tag until maintainer approval

[Unreleased]: https://github.com/joe-broadhead/aurum/compare/v0.0.12...HEAD
[0.0.12]: https://github.com/joe-broadhead/aurum/compare/v0.0.11...v0.0.12
[0.0.11]: https://github.com/joe-broadhead/aurum/compare/v0.0.10...v0.0.11
[0.0.10]: https://github.com/joe-broadhead/aurum/compare/v0.0.9...v0.0.10
[0.0.9]: https://github.com/joe-broadhead/aurum/compare/v0.0.8...v0.0.9
[0.0.8]: https://github.com/joe-broadhead/aurum/compare/v0.0.7...v0.0.8
[0.0.7]: https://github.com/joe-broadhead/aurum/compare/v0.0.6...v0.0.7
[0.0.6]: https://github.com/joe-broadhead/aurum/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/joe-broadhead/aurum/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/joe-broadhead/aurum/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/joe-broadhead/aurum/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/joe-broadhead/aurum/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/joe-broadhead/aurum/compare/v0.0.0...v0.0.1
[0.0.0]: https://github.com/joe-broadhead/aurum/releases/tag/v0.0.0
