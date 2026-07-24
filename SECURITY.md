# Security Policy

## Reporting a Vulnerability

Please report security issues privately through GitHub Security Advisories for
this repository. If advisories are unavailable, contact the maintainer through
the repository owner's GitHub profile before opening a public issue.

Do **not** include vulnerability details, exploit steps, or private logs in
public issues or pull requests.

## Scope notes

Aurum is a on-device speech-to-text CLI and library. Reports are especially welcome for:

- Unexpected network access on the default (`local`) provider path
- API key leakage in logs, debug output, or error messages
- Model download integrity / supply-chain issues
- Temp-file or path handling races
- Memory / resource exhaustion via crafted audio inputs

## Supported versions

Until the first stable (`1.x`) release, security fixes target the latest
published release and the default branch.

## Dependencies

- Local inference uses whisper.cpp via `whisper-rs` (native code)
- Remote provider uses HTTPS to OpenRouter only when explicitly selected
- ffmpeg is a **system** dependency (not bundled in v0.0.0)
