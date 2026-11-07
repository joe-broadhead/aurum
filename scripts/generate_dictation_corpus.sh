#!/usr/bin/env bash
# Generate the dictation eval corpus (evals/dictation) with macOS `say`.
#
# The corpus is dictation-length (1-20 s), covers technical terms, hesitations,
# numbers and two non-US accents, and includes a silence clip for
# false-positive scoring. Every clip is synthetic, so it is redistributable;
# the manifest records the licence per clip.
#
# Requires macOS `say`. No ffmpeg: `say` writes 16 kHz mono PCM WAV directly,
# which is what the STT path and `aurum record` produce anyway.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/evals/dictation/audio"
mkdir -p "$OUT"

need() { command -v "$1" >/dev/null || { echo "missing $1 (macOS only)" >&2; exit 1; }; }
need say
need python3

gen() {
  local id="$1" voice="$2" text="$3"
  local path="$OUT/$id.wav"
  say -v "$voice" -o "$path" --file-format=WAVE --data-format=LEI16@16000 "$text"
  echo "wrote $path ($voice)"
}

gen command_short      Samantha "Open the terminal and run the tests."
gen technical_terms    Samantha "Run cargo test in the aurum workspace, then bump the opencode voice plugin."
gen hesitation         Samantha "Um, so, uh, I think we should, like, maybe ship it today."
gen numbers_time       Samantha "The meeting is at 3:30 pm on the fourteenth of November."
gen punctuation_pause  Samantha "First, install the plugin. Then restart the editor."
gen long_dictation     Samantha "Okay so the plan for today is to finish the voice plugin, run the full test suite, and then write up the release notes for version zero point zero point twenty four. Let me know if anything blocks you."
gen noise_floor        Samantha "Transcribe this clip even with the fan running in the background."
gen quiet_voice        Samantha "This is a quiet microphone check for the voice plugin."
gen code_terms         Samantha "The function parse config returns a result, and the command line prints json."
gen spoken_formatting  Samantha "Let's add a new line and then say quote hello world unquote."
gen accent_gb          Daniel   "Schedule the call for tomorrow morning at half past nine."
gen accent_au          Karen    "I reckon we should check the logs before we deploy."
gen accent_in          Aman     "Please summarise the meeting notes and send them to the team."

# Deterministic post-processing: attenuated speech, added noise, silence.
python3 - "$OUT" <<'PY'
import math
import random
import sys
import wave
from pathlib import Path

out = Path(sys.argv[1])


def read(path):
    with wave.open(str(path), "rb") as w:
        assert w.getnchannels() == 1 and w.getsampwidth() == 2
        rate = w.getframerate()
        frames = w.readframes(w.getnframes())
    samples = [int.from_bytes(frames[i:i + 2], "little", signed=True) for i in range(0, len(frames), 2)]
    return rate, samples


def write(path, rate, samples):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(b"".join(int(max(-32768, min(32767, s))).to_bytes(2, "little", signed=True) for s in samples))


# Quiet microphone: -12 dBFS peak, no noise.
rate, speech = read(out / "quiet_voice.wav")
write(out / "quiet_voice.wav", rate, [int(s * 0.25) for s in speech])

# Noise floor: white noise at ~-30 dBFS mixed under the speech, seeded.
rate, speech = read(out / "noise_floor.wav")
rng = random.Random(20261107)
write(out / "noise_floor.wav", rate, [s + int(rng.gauss(0.0, 1000.0)) for s in speech])

# Silence: one second of digital silence, for false-positive scoring.
write(out / "silence_1s.wav", 16000, [0] * 16000)
PY

(
  cd "$ROOT/evals/dictation"
  find audio -type f -name '*.wav' -print0 | sort -z | xargs -0 shasum -a 256
) >"$ROOT/evals/dictation/SHA256SUMS"
echo "updated evals/dictation/SHA256SUMS"
