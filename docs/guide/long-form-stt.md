# Long-form STT (JOE-2219)

Remote STT for audio longer than the policy target window uses **boundary-aware
chunking**: silence search near the cut, bounded overlap when no quiet region
exists, deterministic overlap deduplication, and honest **timestamp provenance**.

Local whisper full-file behavior is unchanged.

## Policy

`LongFormPolicy` (validated) defaults:

| Field | Default |
|-------|---------|
| target window | 210 s (`AURUM_REMOTE_STT_CHUNK_SECS` may override) |
| silence search | ±15 s around target |
| min silence | 0.25 s |
| non-silent overlap | 1.5 s (capped at 5% of chunk) |

Invalid policy values fail closed at validation.

## Timestamp provenance

Each segment carries `timestamp_source`:

| Value | Meaning |
|-------|---------|
| `native_model` | Local model timing |
| `provider_word` / `provider_segment` | Remote ASR timing |
| `chunk_offset` | Provider timing shifted by chunk start |
| `interpolated` | Soft-split proportional estimate |
| `synthetic_span` | Full-duration span without provider timing |
| `unavailable` | No usable timing |

`timestamps_reliable` is derived **conservatively** from provenance. SRT fails
closed when any segment is approximate unless `--allow-unreliable-timestamps`.

JSON DTO schema version is **2** (v1 still accepted on import).

## Operation semantics

* One cancel flag from the request is checked before each chunk.
* Chunk errors include index and offset (no transcript payload).
* Progress/logs record chunk index and boundary kind only.

## Related

* Implementation: `aurum_core::remote::long_form`, `stt_chunk`
* Quality observatory boundary metrics: JOE-2216
