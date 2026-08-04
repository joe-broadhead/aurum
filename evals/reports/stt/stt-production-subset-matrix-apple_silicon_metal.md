# STT production-subset matrix (JOE-2318)

| Model | Fixtures | mean WER | speech-only WER | silence FP | budget |
|-------|----------|----------|-----------------|------------|--------|
| `tiny-q5_1` | 32 | 0.1864 | 0.1322 | 2 | PASS |
| `base` | 32 | 0.1565 | 0.1003 | 2 | PASS |
| `base.en-q5_1` | 32 | 0.1237 | 0.0987 | 1 | PASS |
| `small-q5_1` | 32 | 0.1501 | 0.0935 | 2 | PASS |

## Honesty

* Real licensed speech (LibriSpeech + noise + multi-accent Common Voice).
* 32-fixture cap — not a full hour sweep of every production fixture.
* Silence FP retained honestly (control-tone hallucinations).
* TED-LIUM / multilingual slots still recipe-only.
* `large-v3-turbo` not scored this cut (disk / time).

