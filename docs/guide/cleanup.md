# Cleanup (flow)

After transcription, Aurum can optionally **clean** the text — the same idea as
ZephyrFlow’s flow styles.

Cleanup is **off by default** (`raw`) so ASR stays verbatim unless you ask.

## Styles

| Style | Rules (on-device) | OpenRouter (LLM) |
|-------|------------------|------------------|
| `raw` | trim only | trim only |
| `clean` | drop fillers, spacing, light punctuation | rewrite cleanly |
| `bullets` | sentence → bullet list | LLM bullets |
| `professional` | expand contractions / casualisms | professional rewrite |
| `summary` | extractive (first + longest sentence) | abstractive summary |

## Providers

| Provider | Network | Default |
|----------|---------|---------|
| `rules` | No | Yes |
| `openrouter` | Yes (`OPENROUTER_API_KEY`) | No |

## Segment policy

After cleaning full text, ASR segments can be:

| Policy | Behavior |
|--------|----------|
| `auto` (default) | **clear** for bullets/summary; **keep** for clean/professional |
| `keep` | leave segments unchanged |
| `clear` | drop all segments |
| `per-segment` | run the same style on each segment text |

## CLI

### With transcription

```bash
aurum talk.m4a --model tiny-q5_1 --cleanup clean
aurum talk.m4a --cleanup bullets
aurum talk.m4a --cleanup clean --cleanup-segments keep

export OPENROUTER_API_KEY=sk-or-...
aurum talk.m4a --cleanup summary --cleanup-provider openrouter
```

### Cleanup only (no audio)

```bash
echo "um, hello there, you know" | aurum cleanup --style clean
aurum cleanup notes.txt --style bullets
aurum cleanup draft.txt --style professional -o json
cat notes.txt | aurum flow -s summary          # alias
```

## Config defaults

```toml
[cleanup]
style = "raw"
provider = "rules"
# openrouter_model = "google/gemini-2.5-flash-lite"
```

CLI flags override the file.

## JSON fields

```json
{
  "text": "Hello world.",
  "cleanup_style": "clean",
  "cleanup_provider": "rules",
  "original_text": "um, hello world",
  "backend_kind": "asr",
  "timestamps_reliable": true
}
```

- `cleanup_style` is always present (`raw` when off).
- `cleanup_provider` and `original_text` appear only when a non-raw cleanup ran.

## Library

```rust
use aurum_core::cleanup::{
    apply_cleanup_with_segments, CleanupStyle, RulesCleanup, SegmentCleanupPolicy, TextCleanup,
};

# async fn demo(mut result: aurum_core::TranscriptionResult) -> aurum_core::Result<()> {
let rules = RulesCleanup::new();
apply_cleanup_with_segments(
    &mut result,
    &rules as &dyn TextCleanup,
    CleanupStyle::Clean,
    SegmentCleanupPolicy::Auto,
).await?;
# Ok(())
# }
```

## Design notes

- **Local-first:** default cleanup backend never leaves the machine.
- **ASR vs flow:** transcription produces text; cleanup is a separate stage.
- **Segments:** structural styles clear segments by default so SRT is not misleading.
