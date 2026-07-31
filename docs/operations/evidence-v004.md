# Evidence pack — v0.0.4 residual (Group A)

Dated: **2026-07-31**  
Hardware profile: **`apple_silicon_metal`** (maintainer Apple Silicon Metal)  
Aurum: **0.0.4**

This document links the first retained, reproducible quality and performance
artifacts after the v0.0.4 product-proof release. It is **not** a human
multi-accent field study and **not** MOS.

## Artifacts

Paths are in the repository root (not under `docs/`):

| Kind | Path |
|------|------|
| Public corpus | `evals/corpus.public-v1.json` |
| Speech audio | `evals/audio/` (+ `SHA256SUMS`) |
| STT matrix | `evals/reports/stt/` |
| TTS objective | `evals/reports/listening/tts-objective-apple_silicon_metal.json` |
| TTS listening pilot | `evals/reports/listening/listening-round-001.json` |
| Perf STT/TTS | `evals/reports/perf/` |

## STT findings (synthetic English corpus)

Models: `tiny-q5_1`, `base`, `small-q5_1` (local, warm Metal).

| Model | mean_wer (all fixtures) | silence false positives |
|-------|-------------------------|-------------------------|
| tiny-q5_1 | ~0.49 | 3/3 silence-like fixtures |
| base | ~0.47 | 3/3 |
| small-q5_1 | ~0.45 | 3/3 |

Observed behaviors:

* **Silence hallucination** — all three models emit short hypotheses on silence
  (`you`) and non-speech tone (e.g. `Thanks for watching!` / `BEEP!`). Tracked
  via `silence_false_positive`.
* **Brand name** — “Aurum” often becomes Oram/Orem; expected for rare tokens.
* **Digits** — spoken letter/digit sequences collapse to compact forms
  (`ABC123`, `5551212`); raw word WER looks high without digit-aware scoring.
* **Clean prose** — fillers / multi-sentence fixtures often WER 0.0 at base/small.

### Budgets (provisional)

| Category | Budget | Action |
|----------|--------|--------|
| Silence FP rate | investigate if > 0 on empty references | do not claim “no hallucination” |
| Speech-only mean WER on this corpus | informational; not a ship gate yet | refresh with licensed human speech before product claims |
| Experimental `large-v3-q5_0` | excluded from normal profiles | keep experimental tier |

## TTS findings

* **Objective:** Kitten and Kokoro samples non-empty, mono, non-clipped, finite PCM.
* **Listening pilot (round 001):** single non-blinded listener; **not MOS**.
  Kokoro scored higher naturalness; Kitten remains **default** (lighter, MIT-safe
  default path). Homographs weak for both.

## Performance (Apple Silicon Metal, warm)

End-to-end CLI wall time on `tests/fixtures/sample.wav` (~5.2 s audio):

| Model | p50 wall | RTF p50 |
|-------|----------|---------|
| tiny-q5_1 | ~0.59 s | ~0.11 |
| base | ~0.80 s | ~0.15 |
| small-q5_1 | ~0.88 s | ~0.17 |

TTS short phrase (warm packs):

| Model | p50 wall | RTF p50 (approx) |
|-------|----------|------------------|
| kitten-nano-int8 | ~0.95 s | ~0.27 |
| kokoro-82m-int8 | ~1.84 s | ~0.89 |

## How to reproduce

```bash
cargo build -p aurum-stt --release
./scripts/generate_eval_speech.sh   # macOS say + ffmpeg
./scripts/generate_eval_audio.py
AURUM_BIN=target/release/aurum python3 scripts/run_stt_eval_matrix.py
# TTS + perf: re-run maintainer scripts or extend run_perf_report.sh
```

## Honest limits

* Corpus speech is **synthetic** (macOS `say` / lavfi), not licensed human speech.
* Multi-accent coverage is **synthetic voice variation**, not real speakers.
* Listening is a **pilot**, not a formal study.
* One hardware profile only; Linux/Windows retained reports still needed.
