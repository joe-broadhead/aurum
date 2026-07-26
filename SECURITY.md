# Security Policy

## Reporting a Vulnerability

Please report security issues privately through GitHub Security Advisories for
this repository. If advisories are unavailable, contact the maintainer through
the repository owner's GitHub profile before opening a public issue.

Do **not** include vulnerability details, exploit steps, or private logs in
public issues or pull requests.

## Scope notes

Aurum is an on-device speech I/O CLI and library (STT + TTS). Reports are especially welcome for:

- Unexpected network access on the default (`local`) STT or TTS path
- API key leakage in logs, debug output, or error messages
- Model / voice-pack download integrity / supply-chain issues (SHA-256 pins)
- Temp-file or path handling races (including TTS atomic WAV writes)
- Path traversal via `--input-file` / `--output-file` on TTS or cleanup
- Memory / resource exhaustion via crafted audio inputs or very large TTS text

## Supported versions

Until the first stable (`1.x`) release, security fixes target the latest
published release and the default branch.

## Dependencies

- Local STT inference uses whisper.cpp via `whisper-rs` (native code)
- Local TTS inference uses ONNX Runtime (`ort`, including vendor prebuilt binaries via `download-binaries`) + KittenTTS weights; G2P via MIT `misaki-rs` (no GPL espeak on the default path). TTS is a default cargo feature and increases binary size; disable with `default-features = false` on `aurum-core` if unused.
- Remote STT provider uses HTTPS to OpenRouter only when explicitly selected
- TTS never calls OpenRouter; network on the TTS path is only for explicit voice/model pack download
- ffmpeg is a **system** dependency for STT file decode (not bundled; not used for TTS WAV)
