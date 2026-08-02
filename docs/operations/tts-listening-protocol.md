# TTS listening protocol (JOE-2217)

Evidence version: **`0.0.22-tts-listening-v1`**

This protocol is lightweight but reproducible. It is **not** a universal MOS
claim. Objective PCM checks remain mandatory and independent.

## Fixture pack

* Schema: `aurum_core::eval::TtsEvalPack` (`schema_version = 1`)
* Built-in pack: `tts_production_pack()` (≥60 utterances)
* Checked-in JSON: `evals/observatory/tts_eval_pack.v1.json`
* Categories: short conversational, punctuation/cadence, numbers/currency/dates/
  times/measurements, abbreviations, proper nouns, homographs, questions,
  exclamations, quotes, US/UK, join/chunk-boundary stress, multi-paragraph
  long-form, very short text, and invalid-input controls

## Objective evaluation (CI-capable)

For each selected model/voice pair record (no synthesis text in public reports):

* Aurum commit, model-pack digest, adapter, trust mode
* sample rate, channels, sample count, duration
* wall time and real-time factor
* chunk count and character count
* peak amplitude, RMS, clipped-sample count
* leading/trailing silence
* join discontinuity at chunk boundaries
* empty/near-empty and truncation flags

API: `score_tts_pcm`, `TtsObjectiveReport`. Negative tests inject clipped,
empty, discontinuous, and truncated PCM and expect failure.

### Required local matrix

| Model | Voices (minimum) |
|-------|------------------|
| Kitten (`kitten-nano-int8`) | default + ≥1 male + ≥1 female |
| Kokoro (`kokoro-82m-int8`) | default + ≥1 US + ≥1 UK |

See `tts_local_matrix()`.

Remote providers run in protected workflows only and do not block local TTS
release readiness when unavailable.

## Human listening protocol

### Requirements

| Rule | Minimum |
|------|---------|
| Independent listeners | **3** |
| Blinding | Randomized model labels; real ids revealed only after scoring |
| Playback | Identical normalization; headphones preferred; same session guidance |
| Utterances per promoted local model | **≥20** representative fixtures |
| Scales | 1–5 for intelligibility, naturalness, pronunciation, join smoothness |
| Critical failure flag | Omitted words, severe mispronunciation, clipping, unusable artifacts |
| Pairwise preference | Optional between default and candidate |
| PII | Random reviewer id only — no name, email, or device serial |

### Capture

1. Generate audio for the selected fixtures offline (local packs only for the
   promotion path).
2. Present clips in random order with blinded labels.
3. Collect ratings into `ListeningRating` records.
4. Map blinded labels → model ids **after** collection.
5. Aggregate with `aggregate_listening` (medians; critical-failure counts).
6. Publish `ListeningReport` aggregates only — never listener identities.

### Support-tier policy

A local model may be documented as **supported** only when:

* all objective safety/correctness checks pass;
* no critical omission/truncation defect in the golden set;
* median intelligibility ≥ **4/5**;
* median pronunciation and join smoothness ≥ **3.5/5**;
* known limitations are documented.

API: `evaluate_support_tier`.

Remote TTS providers require this evidence **plus** the provider live-qualification
issue before a supported label.

### Default model decision

Changing the default requires a separate product decision backed by:

1. this objective + listening report;
2. download/runtime costs;
3. the named-hardware performance report (JOE-2218).

**v0.0.22 Wave 1 disposition:** Kitten remains the default local TTS model.
See `evals/observatory/tts-default-decision.md`.

## Privacy

* Reports: fixture IDs and aggregate scores only.
* Protected remote jobs: synthetic/non-sensitive text only.
* Private provider voice IDs: hashed or reviewed aliases in public reports.
* No listener name, email, or personal device identifier.

## Related

* `docs/guide/tts.md`
* `docs/operations/listening-scorecard.md` (historical pilot form)
* `evals/reports/listening/`
* JOE-2218 performance budgets for synthesis RTF
