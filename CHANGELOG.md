# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **TTS MVP:** `aurum tts` local-default text→mono WAV (ONNX KittenTTS nano int8 + MIT G2P)
  - Nested `aurum tts models` / `aurum tts voices` catalogue + cache status
  - Config `[tts]` + `AURUM_TTS_MODEL` / `AURUM_TTS_VOICE` / `AURUM_TTS_LANGUAGE`
  - Pinned SHA-256 voice pack download under `…/aurum/tts/`, atomic WAV write, `--emit-json` honesty schema
  - `aurum-core` module `tts/` behind cargo feature `tts` (default on for CLI; ORT increases binary size)
  - `LocalTtsProvider::clear_sessions` drops loaded ONNX graphs; wall-clock TTS timeout is best-effort
  - `scripts/generate_tts_demos.sh` — regenerate per-voice demo WAVs locally (not committed)
  - TTS full synth integration test is `#[ignore]` (network/cache); empty-text unit always runs
- `aurum-ffi` crate: C ABI façade (`include/aurum.h`) for embedders — PCM transcribe, preload, cancel, rules cleanup
- Docs: TTS guide (`docs/guide/tts.md`); native embeds guide (`docs/library/ffi.md`); workspace/architecture updated


### Changed

- CLI crates.io package name is **`aurum-stt`** (binary remains `aurum`; the `aurum` crate name is taken)


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

[Unreleased]: https://github.com/joe-broadhead/aurum/compare/v0.0.0...HEAD
[0.0.0]: https://github.com/joe-broadhead/aurum/releases/tag/v0.0.0
