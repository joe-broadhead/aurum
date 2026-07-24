# Changelog

All notable changes to Aurum will be documented here.

## [0.0.0] — unreleased

Initial experimental release foundation.

### Added
- Local transcription via whisper.cpp (`whisper-rs`), Metal-enabled on macOS
- OpenRouter remote provider via multimodal chat completions (`input_audio`)
- CLI: `aurum <file> [--provider] [--model] [--language] [-o txt|srt|json] [--output-file] [--timestamps] [-v]`
- Automatic ggml model download + cache under the platform cache dir
- Config file support (`config.toml`) with env override (`OPENROUTER_API_KEY`)
- Actionable error taxonomy (user / environment / provider)
- Output formatters: plain text, SRT, JSON
- Unit tests, mocked OpenRouter tests, ignored local integration test
- CI workflow for macOS, Linux, Windows

### Notes
- Library API is experimental and may change without notice
- ffmpeg is a required system dependency (not bundled)
- No GitHub Release published yet
