# Aurum

**Aurum** *(Latin: gold)* is a local-first, cross-platform transcription CLI with a clean provider abstraction.

- **Local by default** — whisper.cpp, no API key required  
- **One remote provider** — OpenRouter (LLM-assisted multimodal audio)  
- **Predictable output** — `txt`, `srt`, `json`  
- **Workspace layout** — `aurum-core` library + `aurum` CLI binary  

> Status: **v0.0.0** (experimental). The CLI is usable; the `aurum-core` API is not stable yet.

## Quick start

### Install from source

```bash
# Prerequisites: Rust 1.89+, cmake, a C/C++ toolchain, ffmpeg
# macOS:  brew install cmake ffmpeg
# Ubuntu: sudo apt install build-essential cmake ffmpeg

git clone https://github.com/joe-broadhead/aurum.git
cd aurum
cargo install --path crates/aurum
```

### Transcribe

```bash
# Local (default) — downloads the "base" model on first run (~142 MB)
aurum meeting.m4a

# Fast trial model (~32 MB quantized)
aurum meeting.m4a --model tiny-q5_1

# Subtitles
aurum meeting.m4a --model small-q5_1 -o srt --output-file meeting.srt

# Structured JSON
aurum meeting.m4a -o json

# List models + cache status
aurum models

# Remote via OpenRouter (LLM-assisted — not dedicated ASR)
export OPENROUTER_API_KEY=sk-or-...
aurum meeting.m4a --provider openrouter
```

First local run downloads the selected ggml model into the platform cache dir
(`~/Library/Caches/aurum/models` on macOS, `~/.cache/aurum/models` on Linux) with a progress bar.

## Workspace layout

```text
aurum/
├── crates/
│   ├── aurum-core/     # reusable library (experimental API)
│   └── aurum/          # CLI binary
├── tests/fixtures/     # sample audio
└── README.md
```

Depend on the core from other Rust projects:

```toml
aurum-core = { git = "https://github.com/joe-broadhead/aurum", package = "aurum-core" }
```

## CLI

```text
aurum <AUDIO_FILE> [OPTIONS]
aurum models
aurum transcribe <AUDIO_FILE> [OPTIONS]

Options:
  --provider <local|openrouter>   Default: local
  --model <NAME>                  Local ggml name or OpenRouter model id
  --language <CODE>               e.g. en, auto (default: auto)
  -o, --output <txt|srt|json>     Default: txt
  --output-file <PATH>            Optional explicit output path
  --timestamps                    Include timestamps where available
  --allow-unreliable-timestamps   Force SRT on OpenRouter (not recommended)
  -v, --verbose
  -h, --help
  --version
```

## Providers

### Local (default)

| Detail | Value |
|--------|--------|
| Engine | whisper.cpp via `whisper-rs` |
| Models | full + quantized (`tiny-q5_1`, `base-q5_1`, …) — see `aurum models` |
| Default | `base` |
| Cache | platform cache dir + `/models` |
| GPU | Metal enabled automatically on macOS builds |
| Context | Process-level model cache (reuse across calls in-process) |

### OpenRouter

OpenRouter does **not** expose `/api/v1/audio/transcriptions`. Aurum sends audio through multimodal **chat completions** (`input_audio`).

> **Semantics:** this is **LLM-assisted** transcription, not a dedicated ASR backend.  
> It may paraphrase, drop filler, or invent timestamps. Prefer `--provider local` when verbatim accuracy matters.  
> JSON always sets `timestamps_reliable: false` and `backend_kind: "llm_assisted"`.  
> SRT is refused unless you pass `--allow-unreliable-timestamps`.

| Detail | Value |
|--------|--------|
| Auth | `OPENROUTER_API_KEY` (preferred) or config file |
| Default model | `google/gemini-2.5-flash` |
| Upload | Compressed (mp3 when ffmpeg allows), capped ~24 MB |

## Output formats

| Format | Description |
|--------|-------------|
| `txt`  | Clean plain text (default) |
| `srt`  | SubRip cues with timestamps (local ASR only by default) |
| `json` | `{ text, segments, language, model, provider, duration_secs, backend_kind, timestamps_reliable }` |

## Configuration

Platform config dir + `/aurum/config.toml`  
(e.g. `~/.config/aurum/config.toml` on Linux, `~/Library/Application Support/aurum/config.toml` on macOS).

```toml
[default]
provider = "local"
model = "base"
language = "auto"
output = "txt"

[openrouter]
# api_key = "sk-or-..."
# model = "google/gemini-2.5-flash"
```

**Precedence:** CLI flags > environment variables > config file > built-in defaults.

## Audio handling

- Accepts common formats: mp3, m4a, wav, flac, ogg, …
- Converts to 16 kHz mono PCM when required
- Uses **system ffmpeg** (not bundled)

### Safety limits

| Limit | Default |
|-------|---------|
| Max duration | ~2.25 hours (aligned with PCM budget) |
| Max decoded PCM | ~500 MB (enforced during decode) |
| Max remote upload | ~24 MB compressed |

Whisper special tokens such as `[BLANK_AUDIO]` are stripped. Segment timestamps are clamped to audio duration.

## Library use (experimental)

```rust
use aurum_core::audio::load_audio;
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider};
use std::path::PathBuf;

# async fn example() -> aurum_core::Result<()> {
let audio = load_audio(std::path::Path::new("meeting.m4a")).await?;
let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"));
let result = provider
    .transcribe(&audio, &TranscriptionOptions {
        model: "tiny-q5_1".into(),
        language: "en".into(),
        timestamps: true,
    })
    .await?;
println!("{}", result.text);
# Ok(())
# }
```

## Development

```bash
cargo build -p aurum
cargo test --workspace
cargo run -p aurum -- tests/fixtures/sample.wav --model tiny-q5_1
cargo test -p aurum-core --test local_integration -- --ignored --nocapture
```

## Non-goals (v0.0.0)

- Real-time / microphone streaming  
- Speaker diarization  
- Summarization / LLM post-processing  
- Multiple remote providers  
- GUI / plugins  
- Stable library API guarantees  

## Name

**Aurum** is Latin for *gold*. Soft fallback name if ever needed: `aurum-stt`.

## License

MIT — see [LICENSE](LICENSE).
