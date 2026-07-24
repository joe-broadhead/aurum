# Changelog

All notable changes to Aurum will be documented here.

## [0.0.0] — unreleased

Initial experimental release foundation.

### Added
- Workspace split: `aurum-core` (library) + `aurum` (CLI)
- Local transcription via whisper.cpp (`whisper-rs`, Metal on macOS)
- Process-level `WhisperContext` cache with clean shutdown
- OpenRouter remote provider via multimodal chat completions (`input_audio`)
- Quantized local models (`tiny-q5_1`, `base-q5_1`, `small-q5_1`, turbo/large q5, …)
- `aurum models` — list models and cache status
- Cross-process model download lock (`std::fs::File::lock`)
- JSON fields `backend_kind` + `timestamps_reliable`
- OpenRouter SRT refused unless `--allow-unreliable-timestamps`
- CLI: `aurum <file>`, `aurum transcribe`, `aurum models`
- Automatic ggml model download + cache
- Config file + `OPENROUTER_API_KEY`
- Actionable error taxonomy
- Output: txt / srt / json
- Post-process: strip special tokens, clamp timestamps
- Audio safety limits (duration / decoded size / upload size)
- Unit tests, mocked OpenRouter tests, speech integration test
- CI on macOS / Linux / Windows + macOS integration job

### Notes
- `aurum-core` API is experimental
- ffmpeg is a required system dependency (not bundled)
- OpenRouter path is LLM-assisted, not dedicated ASR
- No GitHub Release published yet
