# Test fixtures

Synthetic speech via macOS `say` + `ffmpeg` (16 kHz mono PCM unless noted).

| File | Purpose |
|------|---------|
| `sample.wav` / `.mp3` / `.m4a` | Happy-path short English |
| `sample_44100_stereo.wav` | 44.1 kHz stereo → ffmpeg convert path |
| `fillers.wav` | Cleanup (`--cleanup clean`) |
| `multi_sentence.wav` | Bullets / summary cleanup |
| `numbers.wav` | Letters + digits |
| `longer.wav` | Slightly longer clip + SRT |
| `silence.wav` | No-speech / empty transcript (~3 s) |

## Regenerate (macOS)

```bash
./scripts/generate_fixtures.sh
```

## Smoke

```bash
cargo run -p aurum-stt --release -- tests/fixtures/sample.wav --model tiny-q5_1
cargo run -p aurum-stt --release -- tests/fixtures/fillers.wav --model tiny-q5_1 --cleanup clean
cargo run -p aurum-stt --release -- tests/fixtures/multi_sentence.wav --model tiny-q5_1 --cleanup bullets
cargo run -p aurum-stt --release -- tests/fixtures/silence.wav --model tiny-q5_1 -o json
```
