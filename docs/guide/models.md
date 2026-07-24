# Models

```bash
aurum models
```

## Recommended

| Name | Approx size | Notes |
|------|-------------|--------|
| `tiny-q5_1` | ~32 MB | Best first-run trial |
| `base-q5_1` | ~60 MB | Good default quantized |
| `base` | ~142 MB | Default full-precision |
| `small-q5_1` | ~190 MB | Higher accuracy, still manageable |

Models are stored under the platform cache:

- macOS: `~/Library/Caches/aurum/models/`
- Linux: `~/.cache/aurum/models/`
- Windows: `%LOCALAPPDATA%\aurum\cache\models\`

First use downloads from Hugging Face (`ggerganov/whisper.cpp`). Cross-process
file locks prevent double downloads. Known models (`tiny`, `tiny-q5_1`) verify
against pinned SHA-256 digests.
