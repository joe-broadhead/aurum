#!/usr/bin/env bash
# Regenerate tests/fixtures (macOS: requires `say` + ffmpeg).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/tests/fixtures"
mkdir -p "$OUT"

need() { command -v "$1" >/dev/null || { echo "missing $1" >&2; exit 1; }; }
need say
need ffmpeg

to_wav16() {
  local src="$1" dest="$2"
  ffmpeg -y -hide_banner -loglevel error -i "$src" -ar 16000 -ac 1 "$dest"
}

echo "Generating fixtures → $OUT"

say -o /tmp/aurum_en.aiff \
  "Hello, this is a test of the Aurum transcription system. One two three four five."
to_wav16 /tmp/aurum_en.aiff "$OUT/sample.wav"
ffmpeg -y -hide_banner -loglevel error -i /tmp/aurum_en.aiff -codec:a libmp3lame -q:a 4 "$OUT/sample.mp3"
ffmpeg -y -hide_banner -loglevel error -i /tmp/aurum_en.aiff -c:a aac -b:a 64k "$OUT/sample.m4a"
ffmpeg -y -hide_banner -loglevel error -i /tmp/aurum_en.aiff -ar 44100 -ac 2 "$OUT/sample_44100_stereo.wav"

say -o /tmp/aurum_fillers.aiff \
  "Um, so, you know, I think we should, uh, maybe postpone the meeting until next week."
to_wav16 /tmp/aurum_fillers.aiff "$OUT/fillers.wav"

say -o /tmp/aurum_multi.aiff \
  "First we need to ship the beta. Second we should write the docs. Third we can plan the release."
to_wav16 /tmp/aurum_multi.aiff "$OUT/multi_sentence.wav"

say -o /tmp/aurum_nums.aiff \
  "The code is A B C one two three. Call me at five five five one two one two."
to_wav16 /tmp/aurum_nums.aiff "$OUT/numbers.wav"

say -o /tmp/aurum_long.aiff \
  "This is a slightly longer sample for Aurum. It includes several sentences so we can check timestamps and subtitle output. The weather is fine and the system should remain stable under a short load."
to_wav16 /tmp/aurum_long.aiff "$OUT/longer.wav"

ffmpeg -y -hide_banner -loglevel error -f lavfi -i anullsrc=r=16000:cl=mono -t 3 \
  -acodec pcm_s16le "$OUT/silence.wav"

ls -lah "$OUT"
echo "Done."
