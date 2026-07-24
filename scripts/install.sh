#!/usr/bin/env bash
# Install the aurum CLI from this repo (source build).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required tool: $1" >&2
    exit 1
  fi
}

need cargo
need cmake
need ffmpeg

echo "Installing aurum CLI from ${ROOT} …"
cargo install --path crates/aurum --locked --force
echo "Installed: $(command -v aurum)"
aurum --version
echo "Tip: run \`aurum models\` then \`aurum your-file.m4a --model tiny-q5_1\`"
