# Cleanup (flow)

After transcription, Aurum can optionally **clean** the text — the same product
idea as ZephyrFlow’s flow styles.

Cleanup is **off by default** (`raw`) so ASR output stays verbatim unless you ask.

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

## CLI

```bash
# On-device cleanup only
aurum talk.m4a --model tiny-q5_1 --cleanup clean
aurum talk.m4a --cleanup bullets
aurum talk.m4a --cleanup professional
aurum talk.m4a --cleanup summary

# LLM cleanup (explicit)
export OPENROUTER_API_KEY=sk-or-...
aurum talk.m4a --cleanup clean --cleanup-provider openrouter
aurum talk.m4a --cleanup summary --cleanup-provider openrouter --cleanup-model google/gemini-2.5-flash
```

## Config defaults

```toml
[cleanup]
style = "raw"              # raw | clean | bullets | professional | summary
provider = "rules"         # rules | openrouter
# openrouter_model = "google/gemini-2.5-flash"
```

CLI flags override the file. Precedence: CLI > config > built-in (`raw` / `rules`).

## JSON fields

When `-o json`:

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

- `cleanup_style` is always present (`raw` when cleanup is off).
- `cleanup_provider` and `original_text` appear only when a non-raw cleanup ran.

## Library

```rust
use aurum_core::cleanup::{apply_cleanup, CleanupStyle, RulesCleanup, TextCleanup};

# async fn demo(mut result: aurum_core::TranscriptionResult) -> aurum_core::Result<()> {
let rules = RulesCleanup::new();
apply_cleanup(&mut result, &rules as &dyn TextCleanup, CleanupStyle::Clean).await?;
# Ok(())
# }
```

## Design notes

- **Local-first:** default cleanup backend never leaves the machine.
- **ASR vs flow:** transcription providers produce text; cleanup is a separate stage
  (same separation as Zephyr’s WhisperEngine vs FlowProcessor).
- **Segments:** cleanup rewrites `result.text` only; SRT segments stay ASR-aligned.
  Prefer `-o txt` or `-o json` after summary/bullets.
