#!/usr/bin/env bash
# Generate local TTS voice demo WAVs (not committed — outputs are build artifacts).
#
# Usage:
#   ./scripts/generate_tts_demos.sh
#   ./scripts/generate_tts_demos.sh --out ~/Downloads/aurum-tts-voices
#   ./scripts/generate_tts_demos.sh --play          # afplay each clip (macOS)
#   TEXT='Custom line.' ./scripts/generate_tts_demos.sh
#
# Requires a built CLI (release preferred):
#   cargo build -p aurum-stt --release
# First run may download the pinned KittenTTS pack (~26 MB) into the Aurum cache.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${OUT:-$ROOT/target/tts-demos}"
PLAY=0
TEXT="${TEXT:-Hello from Aurum. On-device text to speech.}"

# Default KittenTTS pack voice ids (keep in sync with aurum-core tts catalogue).
VOICES=(Bella Jasper Luna Bruno Rosie Hugo Kiki Leo)

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \?//'
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage ;;
    --out|-o)
      OUT="${2:?--out requires a path}"
      shift 2
      ;;
    --play) PLAY=1; shift ;;
    --text)
      TEXT="${2:?--text requires a string}"
      shift 2
      ;;
    *)
      echo "unknown arg: $1 (try --help)" >&2
      exit 2
      ;;
  esac
done

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required tool: $1" >&2
    exit 1
  fi
}

resolve_aurum() {
  if [[ -n "${AURUM_BIN:-}" && -x "$AURUM_BIN" ]]; then
    echo "$AURUM_BIN"
    return
  fi
  if [[ -x "$ROOT/target/release/aurum" ]]; then
    echo "$ROOT/target/release/aurum"
    return
  fi
  if [[ -x "$ROOT/target/debug/aurum" ]]; then
    echo "$ROOT/target/debug/aurum"
    return
  fi
  if command -v aurum >/dev/null 2>&1; then
    command -v aurum
    return
  fi
  echo "aurum binary not found. Build first:" >&2
  echo "  cargo build -p aurum-stt --release" >&2
  echo "Or set AURUM_BIN=/path/to/aurum" >&2
  exit 1
}

AURUM="$(resolve_aurum)"
need mkdir
need file

mkdir -p "$OUT"
echo "Using: $AURUM"
echo "Text:  $TEXT"
echo "Out:   $OUT"
echo

for v in "${VOICES[@]}"; do
  dest="$OUT/${v}.wav"
  echo "→ $v"
  "$AURUM" tts "$TEXT" \
    --voice "$v" \
    --output-file "$dest" \
    --force \
    --emit-json
  file "$dest"
  echo
done

echo "Wrote ${#VOICES[@]} demos under $OUT"
echo "(These WAV files are local artifacts — do not commit them.)"

if [[ "$PLAY" -eq 1 ]]; then
  if ! command -v afplay >/dev/null 2>&1; then
    echo "--play requested but afplay not found (macOS only)." >&2
    exit 1
  fi
  for v in "${VOICES[@]}"; do
    echo "▶ $v"
    afplay "$OUT/${v}.wav"
  done
fi
