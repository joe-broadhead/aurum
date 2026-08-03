# STT production subset scorecard

| Field | Value |
|-------|-------|
| Evidence version | `0.0.22-observatory-v1` |
| Corpus | `aurum-observatory-production-v1` (operator cache) |
| Model | `tiny-q5_1` |
| Provider | local |
| Hardware profile | `apple_silicon_metal` |
| Fixtures scored | 24 (capped subset) |
| Mean WER (all) | 0.1796 |
| Mean WER (speech only) | **0.105** |
| Mean CER | 0.1216 |
| Silence false positives | 2 |
| Mean RTF | 0.0461 |
| Aurum version | 0.0.23 |

## Honesty

* Real licensed speech (LibriSpeech CC BY 4.0 + noise overlays; silence controls
  in-repo). Hypotheses omitted from the JSON report.
* **Not** a full 60-minute / full-matrix claim. Remaining work: full model matrix,
  TED-LIUM / multilingual slots, production budget baseline.
* Machine-readable twin:
  `stt-production-subset-apple_silicon_metal-tiny-q5_1.json`

## Related

* JOE-2318
* `docs/operations/product-proof-residuals.md`
* `docs/operations/stt-production-pack-operator.md`
