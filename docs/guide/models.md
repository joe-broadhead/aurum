# Models

```bash
aurum models
```

Lists the local whisper.cpp catalogue and which files are already cached.

## Recommended

| Name | Approx size | Notes |
|------|-------------|--------|
| `tiny-q5_1` | ~32 MB | Best first-run trial |
| `base-q5_1` | ~60 MB | Good default quantized |
| `base` | ~142 MB | Default full-precision |
| `small-q5_1` | ~190 MB | Higher accuracy, still manageable |
| `turbo` / `large-v3-turbo` | large | Quality / speed tradeoff |

Aliases: `large` → `large-v3`, `turbo` → `large-v3-turbo`.

## Cache location

| Platform | Path |
|----------|------|
| macOS | `~/Library/Caches/aurum/models/` |
| Linux | `~/.cache/aurum/models/` |
| Windows | `%LOCALAPPDATA%\aurum\cache\models\` |

First use downloads from Hugging Face (`ggerganov/whisper.cpp`). Cross-process
locks prevent double downloads. Common models verify against **pinned SHA-256**
digests; all models fail closed on invalid ggml magic.

## Offline / Local Only

```rust
let p = LocalWhisperProvider::new(cache).with_local_only(true);
// Fails with ModelNotCached if the file is missing — no network.
```
