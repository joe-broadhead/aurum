---
name: aurum-batch
description: "Run Aurum bounded resumable multi-file transcription with versioned manifests. Use for folders of lectures, interviews, or voice notes. For single-file STT/TTS and providers, load skills/aurum-speech."
license: MIT
metadata:
  owner: "aurum"
  persona: "batch"
  version: "0.0.21"
---

# Aurum batch skill

## Mission

Process **collections** of audio with one model load, deterministic outputs, and honest partial success.

Provider/model selection follows the same rules as single-file STT — see
**`skills/aurum-speech/`** (local default; remote only with explicit provider + key).

## Flow

1. Dry-run first: `aurum batch ./lectures -O ./out --dry-run`
2. Run: `aurum batch ./lectures -O ./out --model tiny-q5_1` (or `--profile speed`)
3. Resume: `aurum batch ./lectures -O ./out --resume`
4. Retry failures: `aurum batch ./lectures -O ./out --resume --retry-failed`
5. Inspect `./out/aurum-batch-manifest.json`

## Rules

- Not a shell `for` loop — use the native command for governor/transaction semantics.
- Do not clobber an existing manifest without `--resume`.
- Report succeeded/failed/pending counts from the summary.
- Keep outputs under `--output-dir`; do not write transcripts into the source tree by default.
- Do not invent flags; see `aurum batch --help`.

## Load order

- `references/workflow.md`
- `../aurum-speech/references/stt.md` when choosing models/providers
