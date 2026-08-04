# STT production-subset matrix (JOE-2318)

**Disposition:** Done for product-proof residual with honest limits below.

| Model | Fixtures | mean WER | speech-only WER | silence FP | budget |
|-------|----------|----------|-----------------|------------|--------|
| `tiny-q5_1` | 32 | 0.1864 | 0.1322 | 2 | PASS |
| `base` | 32 | 0.1565 | 0.1003 | 2 | PASS |
| `base.en-q5_1` | 32 | 0.1237 | 0.0987 | 1 | PASS |
| `small-q5_1` | 32 | 0.1501 | 0.0935 | 2 | PASS |
| `large-v3-turbo-q5_0` | 32 | 0.0875 | 0.0600 | 1 | PASS |

## Honesty / limits

* Real licensed speech (LibriSpeech + noise + multi-accent Common Voice).
* **32-fixture cap** — not a full-hour sweep of every production fixture.
* Silence FP retained (control-tone hallucinations).
* TED-LIUM / multilingual slots remain recipe-only (not blocking this Done cut).
* Full-precision `large-v3-turbo` not scored (disk); quantized turbo-q5_0 retained.

