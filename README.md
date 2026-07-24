# Aurum

**Aurum** *(Latin: gold)* is a local-first, cross-platform transcription CLI with a clean provider abstraction.

- **Local by default** — whisper.cpp, no API key required  
- **One remote provider** — OpenRouter (multimodal chat audio)  
- **Predictable output** — `txt`, `srt`, `json`  
- **Single binary** — designed for reuse as a library later  

> Status: **v0.0.0** (experimental). The CLI is usable; the library API is not stable yet.

## Quick start

### Install from source

```bash
# Prerequisites: Rust 1.75+, cmake, a C/C++ toolchain, ffmpeg
# macOS:  brew install cmake ffmpeg
# Ubuntu: sudo apt install build-essential cmake ffmpeg

git clone https://github.com/joe-broadhead/aurum.git
cd aurum
cargo install --path .
```

### Transcribe

```bash
# Local (default) — downloads the "base" model on first run (~142 MB)
aurum meeting.m4a

# Smaller/faster local model
aurum meeting.m4a --model tiny

# Subtitles
aurum meeting.m4a --model small -o srt --output-file meeting.srt

# Structured JSON
aurum meeting.m4a -o json

# Remote via OpenRouter
export OPENROUTER_API_KEY=sk-or-...
aurum meeting.m4a --provider openrouter
aurum meeting.m4a --provider openrouter --model google/gemini-2.5-flash
```

First local run downloads the selected ggml model into `~/.cache/aurum/models/` (platform-appropriate cache dir) with a progress bar.

## CLI

```text
aurum <AUDIO_FILE> [OPTIONS]

Options:
  --provider <local|openrouter>   Default: local
  --model <NAME>                  Local ggml name or OpenRouter model id
  --language <CODE>               e.g. en, auto (default: auto)
  -o, --output <txt|srt|json>     Default: txt
  --output-file <PATH>            Optional explicit output path
  --timestamps                    Include timestamps where available
  -v, --verbose
  -h, --help
  --version
```

## Providers

### Local (default)

| Detail | Value |
|--------|--------|
| Engine | [whisper.cpp](https://github.com/ggerganov/whisper.cpp) via [`whisper-rs`](https://crates.io/crates/whisper-rs) |
| Models | `tiny`, `tiny.en`, `base`, `base.en`, `small`, `small.en`, `medium`, `medium.en`, `large-v3`, `large-v3-turbo` (aliases: `large`, `turbo`) |
| Default | `base` |
| Cache | `~/.cache/aurum/models/` (XDG/macOS/Windows equivalents via `directories`) |
| GPU | Metal enabled automatically on macOS builds |

### OpenRouter

OpenRouter does **not** currently expose `/api/v1/audio/transcriptions`. Aurum sends audio through the multimodal **chat completions** API (`input_audio`), which is the supported path for audio-capable models.

> **Semantics:** this is **LLM-assisted** transcription, not a dedicated ASR backend.  
> It may paraphrase, drop filler, or invent timestamps. Prefer `--provider local` when verbatim accuracy matters.

| Detail | Value |
|--------|--------|
| Auth | `OPENROUTER_API_KEY` (preferred) or config file |
| Default model | `google/gemini-2.5-flash` |
| Upload | Compressed (mp3 when ffmpeg allows), capped ~24 MB |
| Failures | Missing key, 401/403, 429 rate limit, and 402 quota errors surface with actionable messages |

```bash
export OPENROUTER_API_KEY=sk-or-...
aurum talk.mp3 --provider openrouter --model openai/gpt-audio-mini
```

## Output formats

| Format | Description |
|--------|-------------|
| `txt`  | Clean plain text (default) |
| `srt`  | SubRip cues with timestamps |
| `json` | `{ text, segments, language, model, provider, duration_secs }` |

## Configuration

File: platform config dir + `/aurum/config.toml`  
(e.g. `~/.config/aurum/config.toml` on Linux, `~/Library/Application Support/aurum/config.toml` on macOS).

```toml
[default]
provider = "local"
model = "base"
language = "auto"
output = "txt"

[openrouter]
# api_key = "sk-or-..."          # prefer OPENROUTER_API_KEY env var
# model = "google/gemini-2.5-flash"
# base_url = "https://openrouter.ai/api/v1"
```

**Precedence:** CLI flags > environment variables > config file > built-in defaults.

## Audio handling

- Accepts common formats: mp3, m4a, wav, flac, ogg, …
- Converts to 16 kHz mono PCM when the engine requires it
- Uses **system ffmpeg** (not bundled). If missing:

```text
macOS:   brew install ffmpeg
Ubuntu:  sudo apt install ffmpeg
Windows: winget install ffmpeg
```

### Safety limits (v0.0.0)

| Limit | Default | Why |
|-------|---------|-----|
| Max duration | 3 hours | Bound RAM before decode finishes |
| Max decoded PCM | ~500 MB | Fail before OOM on pathological input |
| Max remote upload | ~24 MB compressed | Keep base64 JSON payloads workable |

Whisper special tokens such as `[BLANK_AUDIO]` are stripped from output. Segment timestamps are clamped to the audio duration.

## Library use (experimental)

```rust
use aurum::audio::load_audio;
use aurum::providers::{LocalWhisperProvider, TranscriptionOptions, TranscriptionProvider};
use std::path::PathBuf;

# async fn example() -> aurum::Result<()> {
let audio = load_audio(std::path::Path::new("meeting.m4a")).await?;
let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"));
let result = provider
    .transcribe(&audio, &TranscriptionOptions {
        model: "tiny".into(),
        language: "en".into(),
        timestamps: true,
    })
    .await?;
println!("{}", result.text);
# Ok(())
# }
```

The API may change without notice until a stable `0.1.0`.

## Development

```bash
cargo build
cargo test
cargo run -- meeting.wav --model tiny -v
```

### Project layout

```text
aurum/
├── src/
│   ├── main.rs              # binary
│   ├── lib.rs               # reusable core
│   ├── cli.rs
│   ├── config.rs
│   ├── error.rs
│   ├── audio/
│   ├── output/
│   ├── model/
│   └── providers/
│       ├── local.rs         # whisper.cpp
│       └── openrouter.rs
├── tests/
└── .github/workflows/ci.yml
```

### CI

GitHub Actions runs check/test on macOS, Linux, and Windows.

## Distribution (planned)

Single static-ish binaries per platform via GitHub Releases:

- macOS arm64 + x86_64  
- Linux x86_64  
- Windows x86_64  

No Rust toolchain required for end users once release binaries are published.

## Non-goals (v0.0.0)

- Real-time / microphone streaming  
- Speaker diarization  
- Summarization / LLM post-processing  
- Multiple remote providers  
- GUI / plugins  
- Stable library API guarantees  

## Name

**Aurum** is Latin for *gold*. There is no meaningful collision in the STT/CLI space. Soft fallback name if ever needed: `aurum-stt`.

## License

MIT — see [LICENSE](LICENSE).
