#!/usr/bin/env bash
# Generate multi-accent synthetic speech for evals/audio (JOE-1731).
# Requires macOS `say` + ffmpeg. Output is redistributable synthetic speech.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SPEECH="$ROOT/evals/audio/speech"
NOISE="$ROOT/evals/audio/noise"
mkdir -p "$SPEECH" "$NOISE"
need() { command -v "$1" >/dev/null || { echo "missing $1" >&2; exit 1; }; }
need say
need ffmpeg

TEXT="Hello, this is a test of the Aurum transcription system. One two three four five."

gen() {
  local voice="$1" tag="$2"
  local aiff="/tmp/aurum_eval_${tag}.aiff"
  say -v "$voice" -o "$aiff" "$TEXT"
  ffmpeg -y -hide_banner -loglevel error -i "$aiff" -ar 16000 -ac 1 "$SPEECH/clean_${tag}.wav"
  echo "wrote $SPEECH/clean_${tag}.wav ($voice)"
}

gen Samantha en-US
gen Daniel en-GB
gen Karen en-AU

ffmpeg -y -hide_banner -loglevel error \
  -i "$SPEECH/clean_en-US.wav" \
  -filter_complex "anoisesrc=color=white:sample_rate=16000:amplitude=0.02[n];[0][n]amix=inputs=2:duration=first:dropout_transition=0" \
  -ar 16000 -ac 1 "$NOISE/noisy_en-US.wav"
echo "wrote $NOISE/noisy_en-US.wav"

python3 "$ROOT/scripts/generate_eval_audio.py"
(
  cd "$ROOT/evals"
  find audio -type f -name '*.wav' -print0 | sort -z | xargs -0 shasum -a 256
) >"$ROOT/evals/audio/SHA256SUMS"
echo "updated evals/audio/SHA256SUMS"
