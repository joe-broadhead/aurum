# Models

```bash
aurum models
```

Lists the local whisper.cpp catalogue, cache status, and support tier. Every
trusted entry has a reviewed SHA-256 pin and exact size (JOE-1645).

## Recommended (v0.0.3)

| Use case | Model | Notes |
|----------|--------|--------|
| First-run trial | `tiny-q5_1` | ~32 MB, fast |
| Draft / default speed | `base` (default) or `base.en-q5_1` | Balanced size; default prioritizes speed |
| English lecture quality | `small.en` or `large-v3-turbo` | Best observed quality/speed in internal dogfood |
| Higher EN accuracy | `medium.en` | Slower; strong on long EN speech |

**Default remains `base`** unless you opt into a larger model.

### Evidence caveats (not a formal WER programme)

Internal single-clip dogfood compared Aurum local STT to an external consumer
reference on one English lecture (~11 minutes). Findings used for guidance only:

- English-only (`.en`) variants often beat multilingual twins on that clip.
- `small.en` and `large-v3-turbo` were sweet spots; full `large-v3` was not
  automatically better.
- `large-v3-q5_0` is **experimental / not recommended** after a severe
  repetition degeneration on that clip (see below).

This is **not** multi-accent, noisy, or multi-speaker coverage. Do not treat it
as a production WER budget.

## Support tiers

| Tier | Meaning |
|------|---------|
| `supported` | Normal catalogue entry with full SHA-256 authentication |
| `experimental` | Available but not recommended for production quality |

### `large-v3-q5_0` (experimental)

Marked experimental after a pathological repetition loop in dogfood (~21% WER
vs reference with a short phrase repeating dozens of times). Prefer
`large-v3-turbo` or `large-v3-turbo-q5_0` instead. Aurum also emits a
`degeneration_repetition` normalization warning when transcripts show extreme
n-gram loops.

## Catalogue (aliases)

| Name | Approx size | Notes |
|------|-------------|--------|
| `tiny-q5_1` | ~32 MB | Best first-run trial |
| `base-q5_1` | ~60 MB | Good default quantized |
| `base` | ~142 MB | Default full-precision |
| `small` / `small.en` | ~444 MB | Higher accuracy |
| `medium` / `medium.en` | ~1.4 GB | Large download |
| `turbo` / `large-v3-turbo` | ~1.5 GB | Fast large |
| `large-v3-q5_0` | ~1.0 GB | **Experimental — not recommended** |

Aliases: `large` → `large-v3`, `turbo` → `large-v3-turbo`.

## Trust and authenticity

- Every trusted catalogue file has immutable **SHA-256 + exact size** pins.
- Downloads verify-before-publish; wrong digest/size never becomes the final path.
- A local `.sha256` sidecar never authenticates an unpinned artifact.
- Identity is the digest, not a mutable upstream branch tip.

## Cache location

| Platform | Path |
|----------|------|
| macOS | `~/Library/Caches/aurum/models/` |
| Linux | `~/.cache/aurum/models/` |
| Windows | `%LOCALAPPDATA%\aurum\cache\models\` |

First use downloads from Hugging Face (`ggerganov/whisper.cpp`). Cross-process
locks prevent double downloads.

## Offline / Local Only

```rust
let p = LocalWhisperProvider::new(cache).with_local_only(true);
// Fails with ModelNotCached if the file is missing — no network.
```
