# ADR-001: Kokoro-82M TTS adapter feasibility (JOE-1617)

**Status:** Accepted — product-shipped as **opt-in** catalogue model (JOE-1618)  
**Date:** 2026-07-31  
**Epic:** JOE-1576 / GitHub #16  
**Related:** JOE-1615 (adapter contract), JOE-1616 (conformance), JOE-1618 (implementation)

## Context

Users requested a higher-quality TTS option (Kokoro-82M ONNX) alongside KittenTTS.
Aurum must not claim “load any ONNX” support: every engine needs an explicit
adapter that defines tokenizer/G2P, tensors, voices, limits, and output policy.

## Decision

1. **Platform first:** Ship the versioned adapter + model-pack manifest contract
   (`kitten-onnx-v1`, `fake-sine-v1` for conformance, `kokoro-onnx-v0` scaffold).
2. **Kokoro ships as opt-in catalogue model** `kokoro-82m-int8` with adapter
   `kokoro-onnx-v0`, immutable digests, and English voice catalogue (JOE-1618).
3. **No bare ONNX path** is ever treated as a supported model.
4. **Default remains Kitten** (`kitten-nano-int8` / `kitten-onnx-v1`) until a
   separate release decision promotes Kokoro based on measured quality, size,
   latency, and binary impact.

## Research summary

| Topic | Finding |
|-------|---------|
| Upstream | Kokoro ONNX distributions (community Hugging Face packs) vary; Aurum will pin **one** immutable revision with exact digests before shipping |
| License | Model weights typically Apache-2.0; voice/phoneme assets must be checked per file. Default Aurum binary stays MIT path (no GPL espeak) |
| G2P / tokenizer | Kokoro commonly expects phoneme/IPA-style inputs; misaki-rs (MIT) may not cover the full contract — adapter must own mapping or an MIT-compatible alternative |
| Tensors | Exact names/shapes must be verified from a local ORT run, not inferred from filenames (deferred to JOE-1618 proof branch) |
| Sample rate | Expected 24 kHz mono (aligns with existing WAV path) |
| Size / RSS | ~80M parameters → multi-hundred MB pack class; materially larger than Kitten nano int8 (~25 MB) |
| Security | ONNX execution is code-adjacent trust; only `verified` or `builtin` packs are product-supported |

## Adapter mapping (JOE-1615)

| Field | Kokoro (`kokoro-onnx-v0`) |
|-------|---------------------------|
| Adapter id / version | `kokoro-onnx-v0` / 0 |
| Required roles | `onnx`, `voices`, `config` |
| Synthesis supported | **true** (catalogue + local pack) |
| Model id | `kokoro-82m-int8` (int8 ONNX ~88 MB) |
| Languages | `en` / `en-US` / `en-GB` (G2P English) |
| Trust | `builtin` for catalogue; verified digests |
| Sample rate | 24 kHz mono |
| Style shape | `(510, 1, 256)` length-indexed |
| G2P | MIT `misaki-rs` + Kokoro config vocab |

## License matrix (go / no-go)

| Component | License | Redistribution in Aurum cache | Default binary link |
|-----------|---------|-------------------------------|---------------------|
| KittenTTS weights | Apache-2.0 | Yes (pinned) | Yes (download) |
| misaki-rs G2P | MIT | N/A (crate) | Yes |
| ONNX Runtime (`ort`) | MIT/Apache | Vendor prebuilts | Yes (feature `tts`) |
| Kokoro weights | Apache-2.0 (verify per pin) | Only after pin review | Opt-in download only |
| Any GPL phonemizer | GPL | **No** on default path | **No-go** |

**Go** for continued work under JOE-1618 **if and only if**:

- Immutable artifact digests are recorded for every role  
- No GPL-linked dependency is required on the default feature set  
- Conformance suite (JOE-1616) passes on release targets  
- Attribution strings ship in docs and `aurum tts models`

**No-go** remains acceptable: leave `kokoro-onnx-v0` scaffold-only and close
JOE-1618 with rationale if license/G2P/ORT constraints fail.

## Implementation slices (JOE-1618)

1. Local proof-of-synthesis on a throwaway branch (not merged as product support)  
2. Record tensor I/O, voice keys, sequence limits, silence policy  
3. Write pinned `aurum-tts-manifest.json` with digests/sizes/source revision  
4. Implement adapter + catalogue entry with `shipped = true` only after suite pass  
5. Opt-in first; promotion to default is a separate decision  

## Non-goals

- Generic arbitrary ONNX loading  
- Auto-trust of Hugging Face model cards  
- Shipping Kokoro as default in this ADR  

## Pinned artifacts (JOE-1618)

| File | Approx size | Source |
|------|-------------|--------|
| `kokoro-v1.0.int8.onnx` | ~88 MB | thewh1teagle/kokoro-onnx `model-files-v1.0` |
| `voices-v1.0.bin` | ~28 MB | same release (NPZ of per-voice styles) |
| `config.json` | ~2 KB | hexgrad/Kokoro-82M (vocab + model metadata) |

SHA-256 pins live in `crates/aurum-core/src/tts/catalogue.rs` and fail closed on mismatch.

### Tensor contract (local ORT)

| Input | Shape | Type |
|-------|-------|------|
| `tokens` | `[1, sequence_length]` | int64 |
| `style` | `[1, 256]` | float32 |
| `speed` | `[1]` | float32 |
| Output `audio` | `[audio_length]` | float32 @ 24 kHz |

## Evidence checklist

- [x] Local ORT run with recorded tensor metadata  
- [x] Digest/size pins for all artifacts  
- [x] License/attribution in docs + `aurum tts models`  
- [x] Integration smoke (`kokoro_real_synth_from_cache`)  
- [ ] Broader quality corpus vs Kitten (follow-up evals)  
- [ ] Full multi-platform release dogfood  

## Consequences

- Users select `aurum tts --model kokoro-82m-int8 --voice Heart` (opt-in).  
- Kitten remains default; Kokoro download is first-use only.  
- Custom/local packs may use `kokoro-onnx-v0` with verified digests.
