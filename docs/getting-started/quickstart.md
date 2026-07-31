# Quickstart

## First transcript

```bash
# See models + what is already cached
aurum models

# Fast trial (~32 MB quantized download on first use)
aurum path/to/audio.m4a --model tiny-q5_1

# Default local model is `base` (~142 MB full precision)
aurum path/to/audio.m4a

# Subtitles
aurum path/to/audio.m4a --model tiny-q5_1 -o srt --output-file out.srt

# Structured JSON (backend_kind, cleanup_style, segments, …)
aurum path/to/audio.m4a -o json
```

## Repo fixtures

```bash
cargo run -p aurum-stt --release -- tests/fixtures/sample.wav --model tiny-q5_1
cargo run -p aurum-stt --release -- tests/fixtures/fillers.wav --model tiny-q5_1 --cleanup clean
cargo run -p aurum-stt --release -- tests/fixtures/multi_sentence.wav --model tiny-q5_1 --cleanup bullets
```

## Text-to-speech

```bash
aurum tts "Hello from aurum" --output-file /tmp/hello.wav
aurum tts voices
# regenerate all voice demos locally (not committed):
# ./scripts/generate_tts_demos.sh --play
```

See [TTS](../guide/tts.md).

## Batch a folder

```bash
aurum batch ./lectures --output-dir ./out --profile speed
aurum batch ./lectures -O ./out --resume --retry-failed
```

## Profiles (optional)

```bash
aurum models recommend --profile balance
aurum talk.m4a --profile quality
# product default remains `base` when --profile is omitted
```

## Support bundle

```bash
aurum doctor
aurum support-bundle -O /tmp/aurum-support.json
```

## Cleanup (optional)

```bash
# After transcription
aurum talk.m4a --model tiny-q5_1 --cleanup clean
aurum talk.m4a --cleanup bullets

# Text only (no audio)
echo "um, so, you know, hello" | aurum cleanup -s clean
aurum cleanup notes.txt --style professional -o json
```

See [Cleanup](../guide/cleanup.md).

## OpenRouter (optional)

```bash
export OPENROUTER_API_KEY=sk-or-...   # never commit real keys

# Recommended cheap/audio-capable models:
aurum talk.mp3 --provider openrouter --model google/gemini-2.5-flash-lite
aurum talk.mp3 --provider openrouter --model openai/gpt-audio-mini
aurum talk.mp3 --provider openrouter --model mistralai/voxtral-small-24b-2507 -o json
```

!!! warning "LLM-assisted, not dedicated ASR"
    OpenRouter uses multimodal chat completions. Prefer `--provider local` when
    verbatim accuracy matters. SRT is refused unless you pass
    `--allow-unreliable-timestamps`.

## Configuration

Platform config path (via the `directories` crate), e.g. on macOS:

`~/Library/Application Support/aurum/config.toml`

```toml
[default]
provider = "local"
model = "base"
language = "auto"
output = "txt"

[cleanup]
style = "raw"
provider = "rules"

[openrouter]
# api_key — prefer OPENROUTER_API_KEY env var
# model = "google/gemini-2.5-flash-lite"
```

Full reference: [Configuration](../guide/configuration.md).
