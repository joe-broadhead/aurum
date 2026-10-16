#!/usr/bin/env bash
set -euo pipefail

WHISPER_CPP_REVISION="2eeeba56e9edd762b4b38467bab96c2517163158"
OPENAI_WHISPER_REVISION="25639fc17ddc013d56c594bfbf7644f2185fad84"
INESC_REVISION="77837e42b56d4be6ca15a66b5c41c9b8cf3e41b0"
OPENAI_LARGE_V3_REVISION="06f233fe06e710322aca913c1bc4249a0d71fce1"
EXPECTED_F16_BYTES="3095033483"
EXPECTED_Q5_BYTES="1081140203"
EXPECTED_Q5_SHA256="92c6b30b24dc7b035505a1750bfd3dac51d0984ea7120bc518d3f5c3228030c7"

CACHE_ROOT="${XDG_CACHE_HOME:-${HOME}/.cache}"
# Defaults to a directory under CACHE_ROOT once arguments are parsed. Never a
# fixed shared path: this directory holds a Python venv that the script runs.
WORK_DIR=""
# Aurum's own model cache (`directories::ProjectDirs`). On macOS this is
# ~/Library/Caches/aurum and ignores XDG_CACHE_HOME, so it is resolved per
# platform once arguments are parsed rather than derived from CACHE_ROOT.
MODELS_DIR=""
KEEP_F16=0

usage() {
  cat <<'EOF'
Usage: scripts/prepare_portuguese_models.sh [OPTIONS]

Prepare the pinned INESC European Portuguese Whisper checkpoint as Aurum's
trusted Q5_0 cache artifact.

Options:
  --cache-root PATH  cache root for the work dir
                     (default: $XDG_CACHE_HOME or $HOME/.cache)
  --work-dir PATH    conversion workspace, owned by you
                     (default: <cache-root>/aurum/prepare-portuguese)
  --models-dir PATH  Aurum model cache to stage into (default: the directory
                     `aurum models` prints: ~/Library/Caches/aurum/models on
                     macOS, ${XDG_CACHE_HOME:-~/.cache}/aurum/models elsewhere)
  --keep-f16         retain the converted F16 model for quantization comparison
  -h, --help         show this help
EOF
}

while (($#)); do
  case "$1" in
    --cache-root)
      [[ $# -ge 2 ]] || { echo "missing value for --cache-root" >&2; exit 2; }
      CACHE_ROOT="$2"
      shift 2
      ;;
    --work-dir)
      [[ $# -ge 2 ]] || { echo "missing value for --work-dir" >&2; exit 2; }
      WORK_DIR="$2"
      shift 2
      ;;
    --models-dir)
      [[ $# -ge 2 ]] || { echo "missing value for --models-dir" >&2; exit 2; }
      MODELS_DIR="$2"
      shift 2
      ;;
    --keep-f16)
      KEEP_F16=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

for tool in git cmake cc uv ffmpeg; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "required tool not found on PATH: $tool" >&2
    exit 3
  }
done

if command -v sha256sum >/dev/null 2>&1; then
  sha256_file() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  sha256_file() { shasum -a 256 "$1" | awk '{print $1}'; }
else
  echo "required SHA-256 tool not found (sha256sum or shasum)" >&2
  exit 3
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
REQUIREMENTS="${SCRIPT_DIR}/requirements/portuguese-prep.txt"
WORK_DIR="${WORK_DIR:-${CACHE_ROOT}/aurum/prepare-portuguese}"
WHISPER_CPP_DIR="${WORK_DIR}/whisper.cpp"
OPENAI_WHISPER_DIR="${WORK_DIR}/openai-whisper"
CHECKPOINT_DIR="${WORK_DIR}/WhisperLv3-FT"
CONVERT_DIR="${WORK_DIR}/converted"
VENV_DIR="${WORK_DIR}/venv"
F16_PATH="${CONVERT_DIR}/ggml-large-v3-ptpt-f16.bin"
Q5_PATH="${CONVERT_DIR}/ggml-large-v3-ptpt-q5_0.bin"
if [[ -z "$MODELS_DIR" ]]; then
  if [[ "$(uname -s)" == "Darwin" ]]; then
    MODELS_DIR="${HOME}/Library/Caches/aurum/models"
  else
    MODELS_DIR="${XDG_CACHE_HOME:-${HOME}/.cache}/aurum/models"
  fi
fi
MODEL_DIR="$MODELS_DIR"
DEST_PATH="${MODEL_DIR}/ggml-large-v3-ptpt-q5_0.bin"

# Code from the work dir (the venv, the whisper.cpp build) is executed, so it
# must belong to the current user and must not be redirected through a symlink.
require_owned_dir() {
  local dir="$1"
  if [[ -L "$dir" ]]; then
    echo "refusing to use symlinked directory: $dir" >&2
    exit 3
  fi
  (umask 077 && mkdir -p "$dir")
  if [[ ! -d "$dir" || ! -O "$dir" ]]; then
    echo "refusing to use directory not owned by $(id -un): $dir" >&2
    exit 3
  fi
}

require_owned_dir "$WORK_DIR"
mkdir -p "$CONVERT_DIR" "$MODEL_DIR"
available_kib="$(df -Pk "$WORK_DIR" | awk 'NR == 2 {print $4}')"
required_kib=$((13 * 1024 * 1024))
if [[ ! "$available_kib" =~ ^[0-9]+$ ]] || ((available_kib < required_kib)); then
  echo "insufficient free space under ${WORK_DIR}: need at least 13 GiB before conversion" >&2
  exit 3
fi

checkout_pinned_repo() {
  local url="$1" revision="$2" destination="$3"
  if [[ -e "$destination" && ! -d "$destination/.git" ]]; then
    echo "refusing to replace non-Git path: $destination" >&2
    exit 3
  fi
  if [[ ! -d "$destination/.git" ]]; then
    git clone --filter=blob:none --no-checkout "$url" "$destination"
  fi
  git -C "$destination" fetch --depth 1 origin "$revision"
  git -C "$destination" checkout --detach "$revision"
  [[ "$(git -C "$destination" rev-parse HEAD)" == "$revision" ]] || {
    echo "revision verification failed for $destination" >&2
    exit 4
  }
}

checkout_pinned_repo \
  "https://github.com/ggml-org/whisper.cpp.git" \
  "$WHISPER_CPP_REVISION" \
  "$WHISPER_CPP_DIR"
checkout_pinned_repo \
  "https://github.com/openai/whisper.git" \
  "$OPENAI_WHISPER_REVISION" \
  "$OPENAI_WHISPER_DIR"

if [[ -e "$VENV_DIR" ]]; then
  require_owned_dir "$VENV_DIR"
fi
if [[ ! -x "$VENV_DIR/bin/python" ]]; then
  uv venv --python 3.11 "$VENV_DIR"
fi
# Every package, including transitive dependencies, is pinned by hash.
uv pip install --python "$VENV_DIR/bin/python" \
  --require-hashes \
  --index-strategy unsafe-best-match \
  -r "$REQUIREMENTS"

INESC_REVISION="$INESC_REVISION" \
OPENAI_LARGE_V3_REVISION="$OPENAI_LARGE_V3_REVISION" \
CHECKPOINT_DIR="$CHECKPOINT_DIR" \
  "$VENV_DIR/bin/python" - <<'PY'
import os
import shutil
from pathlib import Path
from huggingface_hub import hf_hub_download, snapshot_download

snapshot_download(
    repo_id="inesc-id/WhisperLv3-FT",
    revision=os.environ["INESC_REVISION"],
    local_dir=os.environ["CHECKPOINT_DIR"],
    local_dir_use_symlinks=False,
    ignore_patterns=["*.h5", "*.msgpack", "*.ot"],
)

checkpoint = Path(os.environ["CHECKPOINT_DIR"])
for filename in (
    "added_tokens.json",
    "merges.txt",
    "normalizer.json",
    "special_tokens_map.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "vocab.json",
):
    source = hf_hub_download(
        repo_id="openai/whisper-large-v3",
        revision=os.environ["OPENAI_LARGE_V3_REVISION"],
        filename=filename,
    )
    shutil.copy2(source, checkpoint / filename)
PY

cmake -S "$WHISPER_CPP_DIR" -B "$WHISPER_CPP_DIR/build" \
  -DCMAKE_BUILD_TYPE=Release \
  -DWHISPER_BUILD_EXAMPLES=ON \
  -DWHISPER_BUILD_TESTS=OFF
cmake --build "$WHISPER_CPP_DIR/build" --config Release \
  --target whisper-cli whisper-quantize --parallel

raw_f16="${CONVERT_DIR}/ggml-model.bin"
if [[ ! -f "$F16_PATH" ]]; then
  "$VENV_DIR/bin/python" "$WHISPER_CPP_DIR/models/convert-h5-to-ggml.py" \
    "$CHECKPOINT_DIR" "$OPENAI_WHISPER_DIR" "$CONVERT_DIR"
  mv "$raw_f16" "$F16_PATH"
fi

actual_f16_bytes="$(wc -c < "$F16_PATH" | tr -d '[:space:]')"
if [[ "$actual_f16_bytes" != "$EXPECTED_F16_BYTES" ]]; then
  echo "F16 size mismatch: got $actual_f16_bytes, expected $EXPECTED_F16_BYTES" >&2
  exit 4
fi

"$WHISPER_CPP_DIR/build/bin/whisper-cli" \
  -m "$F16_PATH" \
  -f "$REPO_ROOT/tests/fixtures/sample.wav" \
  -l pt -nt >/dev/null

"$WHISPER_CPP_DIR/build/bin/whisper-quantize" "$F16_PATH" "$Q5_PATH" q5_0
actual_q5_bytes="$(wc -c < "$Q5_PATH" | tr -d '[:space:]')"
actual_q5_sha256="$(sha256_file "$Q5_PATH")"
if [[ "$actual_q5_bytes" != "$EXPECTED_Q5_BYTES" ]]; then
  echo "Q5_0 size mismatch: got $actual_q5_bytes, expected $EXPECTED_Q5_BYTES" >&2
  exit 4
fi
if [[ "$actual_q5_sha256" != "$EXPECTED_Q5_SHA256" ]]; then
  echo "Q5_0 SHA-256 mismatch: got $actual_q5_sha256, expected $EXPECTED_Q5_SHA256" >&2
  exit 4
fi

partial_path="${MODEL_DIR}/.ggml-large-v3-ptpt-q5_0.bin.$$.aurum.partial"
trap 'rm -f "$partial_path"' EXIT
cp "$Q5_PATH" "$partial_path"
chmod 600 "$partial_path"
[[ "$(wc -c < "$partial_path" | tr -d '[:space:]')" == "$EXPECTED_Q5_BYTES" ]]
[[ "$(sha256_file "$partial_path")" == "$EXPECTED_Q5_SHA256" ]]
mv "$partial_path" "$DEST_PATH"
trap - EXIT

if ((KEEP_F16 == 0)); then
  rm -f "$F16_PATH"
fi

cat <<EOF
Prepared and staged large-v3-ptpt-q5_0.

Artifact: $DEST_PATH
Bytes:    $EXPECTED_Q5_BYTES
SHA-256:  $EXPECTED_Q5_SHA256
INESC:    $INESC_REVISION
Tokenizer: $OPENAI_LARGE_V3_REVISION
whisper.cpp: $WHISPER_CPP_REVISION

Use it with Aurum (\`aurum models\` should list it as cached):
  aurum input.wav --model large-v3-ptpt-q5_0 --language pt -o json

Download and use the immutable pt-BR model through Aurum:
  aurum input.wav --model medium-ptbr-q5_0 --language pt -o json
EOF

if ((KEEP_F16 == 1)); then
  echo "Retained F16 for comparison: $F16_PATH"
fi
