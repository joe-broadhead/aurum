# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- PCM-first embedder API: `AudioInput::from_pcm` / `from_pcm_slice`, `WHISPER_SAMPLE_RATE`
- `PcmBuffer` for mic-style accumulate/rolling windows (`pcm` module)
- `LocalWhisperProvider::transcribe_pcm`, `preload`, `is_model_cached` / `is_model_loaded`
- `with_local_only` + `EnsureModelOptions` / `ModelNotCached` (fail closed offline)
- Download progress callbacks (`DownloadProgress` / `with_download_progress`)

## [0.0.0] - 2026-07-24

### Added

- Initial experimental release of **Aurum** (Latin: gold)
- Workspace split: `aurum-core` (library) + `aurum` (CLI)
- Local transcription via whisper.cpp (`whisper-rs`, Metal on macOS)
- Process-level `WhisperContext` cache with pre-exit clear (Metal-safe)
- OpenRouter remote provider (LLM-assisted multimodal chat audio)
- Quantized local models (`tiny-q5_1`, `base-q5_1`, turbo/large q5, …)
- `aurum models` — list models and cache status
- Cross-process model download lock (`std::fs::File::lock`)
- JSON fields `backend_kind` + `timestamps_reliable`
- OpenRouter SRT refused unless `--allow-unreliable-timestamps`
- Automatic ggml model download + cache + magic/pin integrity
- Config file + `OPENROUTER_API_KEY`
- Actionable error taxonomy (user / environment / provider)
- Output: txt / srt / json
- Post-process: strip special tokens, clamp timestamps, NaN guard
- Audio safety limits enforced during decode
- MkDocs Material documentation site
- CI (multi-OS, MSRV 1.89, docs strict, version sync)
- Release workflows (prepare / tag / multi-platform binaries)
- Community files: CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, SUPPORT, AGENTS

### Notes

- `aurum-core` API is experimental until `0.1.0`
- ffmpeg is a required system dependency (not bundled)
- OpenRouter path is LLM-assisted, not dedicated ASR
- crates.io publish is **not** part of automated release yet

[Unreleased]: https://github.com/joe-broadhead/aurum/compare/v0.0.0...HEAD
[0.0.0]: https://github.com/joe-broadhead/aurum/releases/tag/v0.0.0
