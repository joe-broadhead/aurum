# Security notes

See also root [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md).

## Default path

- No API key  
- Network only for **explicit model download** when a ggml file is missing  
- No analytics / telemetry  

## Remote path

- Only when `--provider openrouter` or cleanup provider `openrouter`  
- Key via environment preferred; redacted in `Debug`  
- Uploads compressed and size-capped  

## Integrity

- Model magic verified (fail closed)  
- Pinned SHA-256 for common models (`tiny`, `tiny-q5_1`, `tiny.en-q5_1`, `base`, `base-q5_1`, `base.en-q5_1`, `small-q5_1`)  
- Download stream size cap  
- Cross-process download lock  
- O_EXCL temp files for uploads  

## Reporting

Use GitHub Security Advisories. See root [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md)
for the disclosure rehearsal procedure (JOE-1638).

TTS downloads voice packs only when missing (pinned SHA-256). Local synthesis does not use OpenRouter.

## Supply chain & CI (JOE-1578)

- `cargo audit` / `cargo deny` on every PR (`deny.toml`)
- Actions pinned to commit SHAs (`scripts/check_action_pins.sh`)
- Release SBOM + checksums + provenance — [supply chain](../development/supply-chain.md)
- Threat model / hardening — [threat model](threat-model.md), [hardening](hardening.md)

### TTS BYOM (JOE-1576)

- Prefer built-in catalogue or `trust=verified` packs with exact digests.
- Never pass a bare `.onnx` file as a “model path.”
- `local_unverified` is explicit opt-in only; treat it as running untrusted code
  adjacent to the host process.
- See root [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md)
  and [TTS guide](../guide/tts.md).
