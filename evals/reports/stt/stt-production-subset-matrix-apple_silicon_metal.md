# STT production-subset matrix (JOE-2318)

| Model | Fixtures | mean WER | speech-only WER | silence FP | budget |
|-------|----------|----------|-----------------|------------|--------|
| `tiny-q5_1` | 32 | 0.1864 | 0.1322 | 2 | PASS |
| `base` | 32 | 0.1565 | 0.1003 | 2 | PASS |


## Honesty

* Real licensed speech (LibriSpeech + noise + multi-accent Common Voice).
* 32-fixture cap — not full hour sweep.
* Silence FP=2 retained honestly.
* TED-LIUM / multilingual still recipe-only.

