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

### Quality evidence (JOE-2216 observatory)

The versioned STT quality observatory is the product evidence programme:

- Methodology: `docs/operations/stt-observatory.md`
- Core corpus + budgets: `evals/observatory/`
- Evidence version: `0.0.22-observatory-v1` (`PROFILE_EVIDENCE_VERSION`)
- Retained reports: `evals/reports/stt/`

The redistributable **core** exercises schema, silence controls, and budget
comparison in CI. The **production pack** (≥60 minutes licensed speech, multi
speaker/accent/noise/long-form) is prepared on operator machines from open
licensed sources — never private Plaud material. Profile mappings cite the
reviewed evidence version; explicit `--model` always wins. The global default
remains `base` until a separate reviewed decision changes it.

Corpus evidence is not a claim of universal field quality. Scenario-level
errors matter more than a single aggregate WER.

Historical single-clip dogfood (pre-observatory) still informs guidance only:

- English-only (`.en`) variants often beat multilingual twins on EN lectures.
- `small.en` and `large-v3-turbo` were sweet spots; full `large-v3` was not
  automatically better.
- `large-v3-q5_0` is **experimental / not recommended** after severe repetition
  degeneration on that clip (see below).

## Support tiers

| Tier | Meaning |
|------|---------|
| `supported` | Normal catalogue entry with full SHA-256 authentication |
| `experimental` | Available but not recommended for production quality |

### `large-v3-q5_0` (experimental)

Marked experimental after a pathological repetition loop in dogfood (~21% WER
vs reference with a short phrase repeating dozens of times). Prefer
`large-v3-turbo` or `large-v3-turbo-q5_0` instead. Aurum’s normalization
report API can detect extreme n-gram loops as `degeneration_repetition`;
ordinary CLI/JSON transcript output does not currently surface that event
unless hosts consume `NormalizationReport` directly.

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
