# Eval audio (redistributable)

| Path | Description | License |
|------|-------------|---------|
| `silence_1s.wav` | 1 s mono 16 kHz silence | synthetic CC0 |
| `tone_440_1s.wav` | 1 s 440 Hz tone | synthetic CC0 |
| `speech/clean_en-US.wav` | Synthetic US English | macOS `say` Samantha → wav |
| `speech/clean_en-GB.wav` | Synthetic GB English | macOS `say` Daniel → wav |
| `speech/clean_en-AU.wav` | Synthetic AU English | macOS `say` Karen → wav |
| `noise/noisy_en-US.wav` | US speech + white noise | synthetic + lavfi |

Checksums: `SHA256SUMS`.

**Not human speech.** Suitable for offline regression and silence-hallucination
smoke tests. Replace with licensed human multi-accent speech for production WER claims.

Regenerate:

```bash
./scripts/generate_eval_audio.py
./scripts/generate_eval_speech.sh   # macOS
```
