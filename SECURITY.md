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
- Temp-file or path handling races (STT/cleanup/TTS share one output transaction)
- Path traversal via `--input-file` / `--output-file` on TTS or cleanup
- Memory / resource exhaustion via crafted audio inputs or very large TTS text
- Symlink-destination clobber or partial-file exposure on crash mid-write

## Supported versions

Until the first stable (`1.x`) release, security fixes target the latest
published release and the default branch.

## Dependencies

- Local STT inference uses whisper.cpp via `whisper-rs` (native code)
- Local TTS inference uses ONNX Runtime (`ort`, including vendor prebuilt binaries via `download-binaries`) + KittenTTS weights; G2P via MIT `misaki-rs` (no GPL espeak on the default path). TTS is a default cargo feature and increases binary size; disable with `default-features = false` on `aurum-core` if unused.
- Remote STT provider uses HTTPS to OpenRouter only when explicitly selected
- TTS never calls OpenRouter; network on the TTS path is only for explicit voice/model pack download
- ffmpeg is a **system** dependency for STT file decode (not bundled; not used for TTS WAV)

## External I/O trust (JOE-1572)

- **FFmpeg:** shell-free argv, `-nostdin`, concurrent pipe drain, stderr tail cap,
  wall-clock deadline, kill-on-failure. Protocol whitelist prefers local files.
  Hostile multi-tenant media still requires an outer process sandbox.
- **Remote HTTP:** credentialed traffic defaults to the official OpenRouter HTTPS
  origin only. Custom endpoints require `openrouter.allow_custom_endpoint = true`.
  Redirects are disabled; system proxy is off by default.
- **Models:** downloads stream to exclusive partials, hash while writing, and
  publish only after reviewed size/digest checks. Cache inventory:
  `aurum cache status|verify`.

## API contracts & secrets (JOE-1575)

- Config diagnostics and `Debug` never print API keys (redacted as `***`).
- Prefer environment credentials over plaintext file keys.
- Capability preflight rejects impossible routes before network/decode work.
- External JSON DTOs exclude raw PCM and native handles.

## TTS model packs & BYOM trust (JOE-1576)

- **No bare ONNX support.** A pack must declare a known adapter id plus artifacts
  in `aurum-tts-manifest.json`. Filename heuristics and Hugging Face cards are
  never treated as a support guarantee.
- **Trust modes:** `builtin` (catalogue pins), `verified` (exact digests/sizes),
  `local_unverified` (explicit opt-in only; metadata marks unsupported). ONNX
  execution is **code-adjacent trust** — unverified packs are the caller's
  responsibility.
- Local packs never shadow built-in cache identities. Symlink pack roots are
  rejected. Artifact size caps apply before load.
- Custom catalogue entries (`[[tts.custom_models]]`) cannot use reserved built-in
  ids or `trust=builtin`. Remote auto-trust of arbitrary repositories is not
  supported.
- Inspect/verify tooling is read-only by default; `add` is dry-run unless
  `--write-manifest` is explicit.

## Performance & streaming memory (JOE-1574)

- PCM ring buffers never allocate beyond configured capacity; invalid floats are
  rejected before native DSP.
- FFmpeg decode converts s16le chunks to f32 on the fly (no dual full raw+f32
  buffers). Remote dedicated STT streams multipart from disk without a full
  base64 intermediate.
- Output formatters enforce a hard byte budget before publish.

## Runtime concurrency & resource governance (JOE-1573)

- Process lifecycle admission blocks new work after shutdown begins; context
  caches clear only when the active-op count is proven zero.
- ResourceGovernor enforces permits for model loads, local STT/TTS, remote jobs,
  blocking work, CPU threads, and soft memory reservations. Overload yields a
  typed error rather than unbounded host exhaustion.
- Singleflight coalesces concurrent cold loads of the same model key.
- TTS caller timeouts are soft deadlines: native ONNX work remains tracked and
  holds permits until it returns (hard-kill requires an outer process sandbox).

## Output file commits

User-visible output files (transcripts, cleanup text, TTS WAV) use a shared
commit protocol in `aurum_core::output::transaction`:

- Randomized same-directory temporary file with exclusive create
- Write → flush → `sync_all` on the temp file
- Atomic publish (`rename`; Windows removes destination first)
- Best-effort parent-directory sync
- Default **reject** policy for destination symbolic links
- TTS supports `NoClobber` (default without `--force`) and `Replace` (`--force`)

A failure before publish leaves the previous destination intact and removes the
temp file. Concurrent writers never share a temp path.
