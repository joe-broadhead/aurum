# Text-to-speech (TTS)

Aurum turns **UTF-8 text → mono WAV** on-device by default (no API key).

```bash
aurum tts "Hello from aurum" --output-file /tmp/a.wav
aurum tts --input-file prompt.txt -O /tmp/a.wav --emit-json
aurum tts models
aurum tts voices
```

TTS is a **subcommand only** — it does not change STT defaults (`aurum <file>`, `aurum models`, `aurum cleanup`).

## Quick start

1. Build the CLI (`cargo build -p aurum-stt --release` or install from source).
2. First synthesis downloads the pinned English voice pack (~26 MB) into the Aurum cache (`…/aurum/tts/`).
3. Subsequent runs are offline when the pack is cached.

```bash
aurum tts "Hello from aurum" --output-file /tmp/hello.wav --emit-json
file /tmp/hello.wav   # RIFF WAVE, mono PCM
```

### Agent / command-provider template

```bash
aurum tts --input-file {input_path} --voice {voice} --language {language} \
  -o wav --output-file {output_path} --emit-json
```

### Voice demos (local only — not in git)

Regenerate a short WAV per catalogue voice under `target/tts-demos/` (gitignored via `/target`):

```bash
cargo build -p aurum-stt --release
./scripts/generate_tts_demos.sh
./scripts/generate_tts_demos.sh --play          # macOS: afplay each clip
./scripts/generate_tts_demos.sh --out ~/Downloads/aurum-tts-voices
TEXT='Custom line.' ./scripts/generate_tts_demos.sh
```

Do **not** commit the generated WAVs; they are rebuildable artifacts of the pinned model pack.

### Integration test

```bash
cargo test -p aurum-core --test tts_synth -- --ignored --nocapture
```

The full synth test is `#[ignore]` by default (may download the voice pack). Empty-text validation always runs in `cargo test`.

## CLI

| Flag | Required | Notes |
|------|----------|-------|
| `TEXT` / `-` / `--input-file` | exactly one | UTF-8 |
| `--provider` | no | only `local` |
| `--model` | no | default `kitten-nano-int8` |
| `--voice` | no | default `Luna` |
| `--language` | no | default `en` |
| `-o/--output` | no | only `wav` |
| `--output-file` / `-O` | **yes** for synth | destination path |
| `--force` | no | allow overwrite of existing non-empty file (replace mode) |
| `--speaking-rate` | no | must be finite in `0.5..=2.0` (rejected otherwise) |
| `--cleanup` | no | rules-only `raw` \| `clean` |
| `--timeout` | no | milliseconds (default `120000`); see [Timeouts](#timeouts) |
| `--local-only` | no | fail if pack missing (no download) |
| `--emit-json` | no | honesty JSON on stdout; audio only in file |
| `-v` | no | verbose |

Nested catalogue commands (do not collide with STT `aurum models`):

```bash
aurum tts models
aurum tts voices
```

### Exit codes

Same taxonomy as STT: `0` ok, `2` user, `3` environment, `4` provider, `1` internal.

### I/O rules

- Shared secure output transaction (exclusive same-directory temp → flush/sync → atomic publish).
- Refuse overwrite of an existing non-empty file without `--force` (`NoClobber`); `--force` uses `Replace`.
- Destination symbolic links are rejected by default.
- Empty / whitespace-only text → exit `2`.
- Max characters default `5000` (config `[tts].max_chars`); longer input is a **user error** (complete-or-error — no silent truncation).
- Long text within `max_chars` is **chunked** by model phoneme capacity (sentence, then word boundaries) with a short silence between chunks; an unsplittable unit that exceeds the model limit fails with a precise error.
- Sample rate is always the adapter native rate (24 kHz for KittenTTS). Arbitrary rate overrides are rejected; real resampling is not implemented.

## Honesty JSON (`--emit-json`)

Stdout is **only** JSON metadata (no audio bytes). Model/voice IDs are **canonical** catalogue IDs actually used:

```json
{
  "backend_kind": "local",
  "provider": "local",
  "model": "kitten-nano-int8",
  "voice": "Luna",
  "language": "en",
  "output_path": "/abs/path/out.wav",
  "format": "wav",
  "sample_rate_hz": 24000,
  "channels": 1,
  "duration_ms": 1234,
  "text_chars": 42,
  "text_truncated": false,
  "chunk_count": 1,
  "synthesized_chars": 42
}
```

## Configuration

```toml
[tts]
provider = "local"
model = "kitten-nano-int8"
voice = "Luna"
language = "en"
max_chars = 5000
timeout_ms = 120000
```

Environment overrides (no secrets required):

| Variable | Maps to |
|----------|---------|
| `AURUM_TTS_MODEL` | `[tts].model` |
| `AURUM_TTS_VOICE` | `[tts].voice` |
| `AURUM_TTS_LANGUAGE` | `[tts].language` |

## Offline / cache

- Pack cache: `<aurum-cache>/tts/<model-id>/` (onnx + voices.npz + config.json).
- Downloads use a cross-process advisory lock and **pinned SHA-256** (fail-closed on mismatch).
- Warm cache + `--local-only` (or offline network): synthesis uses only local files.
- Cold cache offline: fails closed with a user/provider error (no silent stub audio).

## Voices

Default pack ships eight English aliases: `Bella`, `Jasper`, `Luna` (default), `Bruno`, `Rosie`, `Hugo`, `Kiki`, `Leo`.

## License matrix

| Component | SPDX / terms | Notes |
|-----------|--------------|-------|
| Aurum code | **MIT** | This repository |
| ONNX Runtime (`ort` crate + prebuilt binaries) | MIT / Apache-2.0 | Linked at runtime; not GPL |
| G2P (`misaki-rs`, default features **off**) | MIT | No espeak-ng / GPL phonemizer on the default path |
| KittenTTS nano int8 weights | Apache-2.0 | Hugging Face `KittenML/kitten-tts-nano-0.8-int8` |
| Voice embeddings (`voices.npz`) | Apache-2.0 | Same pack |

**Not linked into the default binary:** eSpeak NG (GPL), Piper phonemize (espeak-backed), ffmpeg (not used for TTS WAV).

### Pinned default pack

| File | SHA-256 |
|------|---------|
| `kitten_tts_nano_v0_8.onnx` | `f7b0afcbee92870b32b8e0276d855b954dc25470c9f051b376ac7eee537c76fc` |
| `voices.npz` | `8aa7cee235abb0739cb51e6559685f65a4dacd95568833d05699b1633f519b3f` |
| `config.json` | `b66006ccbeccd4de5fc3c9272059c47f5725df7215fd889785c03602652fab64` |

Source: `https://huggingface.co/KittenML/kitten-tts-nano-0.8-int8`

## Engine notes

- **Why KittenTTS (not Piper default):** Piper-class pipelines commonly need eSpeak NG for phonemes (GPL). KittenTTS ONNX + MIT G2P keeps the MIT binary story intact while still delivering real neural speech.
- Sample rate: **24 kHz** mono PCM 16-bit WAV (in-process via `hound`). Duration is derived from final PCM length after chunk concat and trailing-silence validation.
- Trailing silence is trimmed with a bounded energy detector (never empties short valid clips). Fixed sample chopping is not used.
- Voices are **model-scoped**: a voice only works with its catalogue model pack.
- Peak/loudness guard on PCM before write (no NaN).
- Cargo feature: `aurum-core` feature `tts` (default **on** for the CLI).

### Binary size / dependencies

Enabling TTS (the CLI default) pulls in **ONNX Runtime** via the `ort` crate (`download-binaries`). That increases build artifacts and release binary size versus an STT-only build. Library consumers who only need STT can disable it:

```toml
aurum-core = { version = "0.0.2", default-features = false }
# or: default-features = false, features = []  — STT only
```

The CLI package keeps `tts` on so `aurum tts` works out of the box.

### Timeouts

`--timeout` / `[tts].timeout_ms` bounds how long Aurum **waits** for a synthesis worker (`spawn_blocking` + `tokio::time::timeout`).

- On expiry the CLI returns a **provider** error and sets a cancel flag when one was provided.
- This is **best-effort**: ONNX Runtime work already running inside the blocking thread is not hard-killed; CPU may continue briefly until the worker returns.
- Do not assume wall-clock cancel frees the core the instant the timeout fires (same class of limitation as many embed runtimes).

Library hosts that need deterministic teardown should drop the `LocalTtsProvider` (and call `LocalTtsProvider::clear_sessions` when done) after cancelling in-flight work at the application layer.

## Limits (MVP)

- English default voice only (additional languages out of scope).
- WAV only (no ogg/mp3/ffmpeg).
- No remote TTS, streaming, mic playback, voice cloning, or FFI surface for TTS.
