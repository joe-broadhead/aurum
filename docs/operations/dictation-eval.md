# Dictation eval corpus (#11)

Aurum's production STT observatory is long-form and lecture oriented
(`stt-observatory.md`). Dictation is a different job: one to twenty seconds, one
speaker, code terms, hesitations, a noisy fan, and a keyboard-replacement
latency budget. `evals/dictation/` is that pack, and it is the corpus the
`opencode-voice` fixtures are copied from.

## Corpus

| | |
|---|---|
| Manifest | `evals/dictation/corpus.v1.json` |
| Audio | `evals/dictation/audio/*.wav` (16 kHz mono PCM) |
| Clips | 14 (13 speech, 1 silence) |
| Duration | 1.0 s – 11.0 s, ~52 s total |
| Speech | macOS `say` (Samantha, Daniel en-GB, Karen en-AU, Aman en-IN) |
| Licence | CC0-1.0, synthetic; no human recordings |
| Generator | `scripts/generate_dictation_corpus.sh` |
| Integrity | `evals/dictation/SHA256SUMS` |

The clips cover: a short command, technical/code terms (`cargo`, `aurum`,
`opencode`, `parse`, `json`), fillers (`um`, `uh`, `like`), a time and a date,
two sentences with punctuation, an 11 s paragraph, a fan-noise floor, a −12 dB
quiet microphone, spoken formatting words, three accents, and one second of
digital silence for false-positive scoring.

Adding a clip: edit the manifest, run the generator, commit the WAV and
`SHA256SUMS`. The generator writes 16 kHz mono WAV directly; no ffmpeg.

## Harness

```bash
make eval-dictation-check     # offline structure only (default CI gate)
make eval-dictation           # real WER run; needs a built binary + cached model
```

`make eval-dictation` honours `AURUM_CACHE` (the model store) and
`AURUM_EVAL_MODEL` (default `tiny-q5_1`):

```bash
AURUM_CACHE=~/.cache/voice/aurum AURUM_EVAL_MODEL=tiny-q5_1 make eval-dictation
```

The report lands in `evals/reports/_local/dictation-<model>.json` (untracked)
and carries, per clip: `wer`, `cer`, `vocabulary_hit/total`, `rtf`, the
hypothesis and the reference.

### Metrics

| Metric | Definition | Budget (v1) |
|---|---|---|
| Mean WER | word edit distance / reference words, averaged over speech clips | ≤ 0.25 |
| Per-clip WER | any clip above the cap fails | ≤ 0.60 |
| Vocabulary recall | expected terms present as token subsequences | ≥ 0.70 |
| Silence false positives | non-empty hypothesis for the silence clip | ≤ 1 |
| Silence inserted words | total words hallucinated on silence | ≤ 1 |
| RTF | wall seconds / audio seconds, **including process start and model load** | reported, not gated |

### Baseline (2026-11-07)

`tiny-q5_1`, Metal, macOS arm64: mean WER **0.175**, CER **0.079**,
vocabulary recall **0.769**, 14/14 clips, ~1 false-positive word on digital
silence, mean wall RTF ~0.4. Per-clip WER runs from 0.000 (command, code terms,
punctuation, en-AU) to 0.417 (numbers/date phrasing) — the tiny model's known
weak spots, which is why the budget is a regression floor and not a quality
claim. `large-v3-turbo` and `base` can be measured with `AURUM_EVAL_MODEL`.

RTF here is per-process: each clip spawns `aurum`, which loads the model. It is
a cold-call number, not a decode-speed claim; a warm sidecar transcribes a 2 s
clip in ~0.17 s (`docs/guide/live.md`).

### The silence clip

`whisper-tiny` emits the single token "you" for a file of digital silence. That
is a real behaviour, not a harness artefact, and it is why the budget permits
exactly one inserted word: a dictation host gates on speech before it calls
`transcribe`, and a neural VAD is the follow-up (#2 in `AURUM-FROM-HEX.md`).
Any second false positive, or a longer hallucination, fails the gate.

Normalization is `aurum.dictation.normalize.v1`: lowercase, drop punctuation and
apostrophes, collapse whitespace. The manifest records the policy and the
harness refuses a corpus that names a different one.

Budgets are a regression floor for the shipped default model, not a quality
claim. A budget change is a reviewed decision and belongs in the changelog, the
same way a model change does.

## CI

`ci.yml` job **`dictation-eval`** (macOS 14, Metal) builds the release CLI,
downloads the pinned model into a private `AURUM_CACHE`, runs the harness and
uploads the report. It fails the build when a budget is exceeded, so a model or
decoder change cannot silently regress dictation.

The offline `--check` also runs in `make ci` and in the `eval-dictation` test
segment, so a broken manifest fails fast without a model.
