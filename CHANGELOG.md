# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

[Unreleased]: https://github.com/joe-broadhead/aurum/compare/v0.0.2...HEAD
[0.0.2]: https://github.com/joe-broadhead/aurum/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/joe-broadhead/aurum/compare/v0.0.0...v0.0.1
[0.0.0]: https://github.com/joe-broadhead/aurum/releases/tag/v0.0.0
