# Local STT

```bash
aurum meeting.m4a
aurum meeting.m4a --model tiny-q5_1
aurum meeting.m4a --profile speed
aurum meeting.m4a -o srt --output-file meeting.srt
aurum meeting.m4a --cleanup clean
```

- Default provider: `local` (whisper.cpp).
- Default model: `base` (unchanged by profiles unless `--profile` is passed).
- `--model` always overrides `--profile`.
- ffmpeg is required for non-16 kHz mono WAV inputs.
- Do not pass API keys for local STT.
