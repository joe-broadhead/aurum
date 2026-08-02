# Migration notes (0.0.3 contracts)

## Errors

- Prefer `AurumError` alias (`type AurumError = TranscriptionError`).
- Use `error_category()`, `retryable()`, and refined `exit_code()` (5=cancel,
  6=deadline, 7=overload).
- New: `UserError::UnsupportedCapability` from preflight.

## Config

- Call `Config::validate()` after CLI merge for library hosts.
- Explicit missing file: `Config::load_from_required`.
- Diagnostics: `effective_diagnostic()` (secrets as `***`).

## PCM

- `PcmBuffer::samples()` returns `ContiguousSamples` — use `.as_slice()` for
  `&[f32]` APIs.

## JSON

- STT output uses `SttResultDto` (`schema_version: 1`), including
  optional `normalization_warnings`.
- TTS metadata: `TtsMetaDto` (no PCM in JSON). Do not `Deserialize`
  `SynthesisResult` expecting PCM.
- **JOE-1937:** `TtsMetaDto.backend_kind` may be `"local"` or `"remote"`.
  `schema_version` remains **1** (the field was already a free string; only
  the documented value set expands). Local CLI honesty JSON now uses
  `result.backend_kind` instead of a hardcoded `"local"`.

## Cleanup

- English filler/contraction heuristics only when language is English/`auto`.
- Ambiguous expansions (`I'd`, `it's`) are **not** rewritten.
- Paragraph breaks (`\n\n`) preserved in `clean` / `professional`.

## Providers

- Prefer `preflight_stt` / `preflight_tts` / `preflight_cleanup` before work.
- OpenRouter SRT with LLM-assisted paths fails preflight when timestamps are
  unreliable.
