#!/usr/bin/env bash
# Qualify a **downloaded/extracted** native SDK archive (JOE-2225).
#
# Does not link against the workspace target/ directory. Builds bundled C11
# (and C++17 when a C++ toolchain is present) examples using only bundle paths.
#
# Usage:
#   ./scripts/qualify_native_sdk_bundle.sh --archive dist/native-sdk/aurum-sdk-*.tar.gz
#   ./scripts/qualify_native_sdk_bundle.sh --dir /path/to/extracted/aurum-sdk-...
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

ARCHIVE=""
DIR=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) ARCHIVE="$2"; shift 2 ;;
    --dir) DIR="$2"; shift 2 ;;
    -h|--help)
      echo "Usage: $0 --archive FILE.tar.gz | --dir EXTRACTED_SDK"
      exit 0
      ;;
    *) echo "unknown: $1" >&2; exit 2 ;;
  esac
done

# Stage under the repo (not /tmp): Windows GHA bash + system Python mishandle /tmp paths.
mkdir -p "${ROOT}/dist"
WORKDIR="$(mktemp -d "${ROOT}/dist/.sdk-qual-XXXXXX")"
WORKDIR="$(cd "${WORKDIR}" && pwd -P 2>/dev/null || pwd)"
trap 'rm -rf "$WORKDIR"' EXIT

if [[ -n "$ARCHIVE" ]]; then
  # Resolve archive to absolute path for mixed bash/Python environments.
  if [[ "${ARCHIVE}" != /* && ! "${ARCHIVE}" =~ ^[A-Za-z]:[\\/] ]]; then
    ARCHIVE="${ROOT}/${ARCHIVE}"
  fi
  tar -xzf "$ARCHIVE" -C "$WORKDIR"
  # Expect single top-level dir
  DIR="$(find "$WORKDIR" -mindepth 1 -maxdepth 1 -type d | head -1)"
elif [[ -n "$DIR" ]]; then
  # Copy so we never depend on caller's paths mutating
  cp -R "$DIR" "$WORKDIR/sdk"
  DIR="$WORKDIR/sdk"
else
  echo "need --archive or --dir" >&2
  exit 2
fi
DIR="$(cd "$DIR" && pwd -P 2>/dev/null || pwd)"

echo "== qualify SDK at $DIR =="

# Path traversal / unexpected symlinks
DIR_ENV="$DIR" python3 - <<'PY'
from pathlib import Path
import os, sys
root = Path(os.environ["DIR_ENV"]).resolve()
for p in root.rglob("*"):
    if p.is_symlink():
        print(f"FAIL unexpected symlink: {p}", file=sys.stderr)
        sys.exit(1)
    try:
        p.resolve().relative_to(root)
    except ValueError:
        print(f"FAIL path escapes root: {p}", file=sys.stderr)
        sys.exit(1)
print("OK no path traversal/symlinks")
PY

need() {
  local f="$1"
  if [[ ! -e "$DIR/$f" ]]; then
    echo "missing required path: $f" >&2
    exit 1
  fi
}

need include/aurum.h
need examples/job_cleanup.c
need examples/engine_raii.cpp
need SDK_MANIFEST.json
need BUILD.md

# Manifest self-check
DIR_ENV="$DIR" python3 - <<'PY'
import json, hashlib, sys, os
from pathlib import Path
root = Path(os.environ["DIR_ENV"])
man = json.loads((root / "SDK_MANIFEST.json").read_text())
assert man.get("schema_version") == 1
assert man.get("abi_version") == 2
assert man.get("remote_via_c_abi") is False
assert man.get("aurum_version")
# Verify digests for every listed file except the manifest itself may lag
files = man.get("files") or {}
assert "include/aurum.h" in files
assert any(k.startswith("lib/") for k in files), "no lib/* in manifest"
for rel, meta in files.items():
    if rel == "SDK_MANIFEST.json":
        continue
    p = root / rel
    if not p.is_file():
        print(f"FAIL missing file from manifest: {rel}", file=sys.stderr)
        sys.exit(1)
    h = hashlib.sha256(p.read_bytes()).hexdigest()
    if h != meta.get("sha256"):
        print(f"FAIL digest mismatch: {rel}", file=sys.stderr)
        sys.exit(1)
    if int(meta.get("size", -1)) != p.stat().st_size:
        print(f"FAIL size mismatch: {rel}", file=sys.stderr)
        sys.exit(1)
# Header ABI constants
hdr = (root / "include/aurum.h").read_text()
assert "#define AURUM_ABI_VERSION 2" in hdr
assert "#define AURUM_ABI_MIN_VERSION 2" in hdr
assert "#define AURUM_SAMPLE_RATE 16000" in hdr
print("OK SDK_MANIFEST + header ABI")
PY

# Locate static library
LIB=""
for cand in lib/libaurum_ffi.a lib/aurum_ffi.lib; do
  if [[ -f "$DIR/$cand" ]]; then
    LIB="$DIR/$cand"
    break
  fi
done
if [[ -z "$LIB" ]]; then
  echo "no static library found under lib/" >&2
  exit 1
fi

# Build C11 example without any workspace target path
OUT="$WORKDIR/bin"
mkdir -p "$OUT"
INC="$DIR/include"
EX="$DIR/examples/job_cleanup.c"

echo "== compile C11 job_cleanup from bundle only =="
case "$(uname -s)" in
  Darwin)
    cc -std=c11 -I "$INC" "$EX" "$LIB" \
      -lpthread -ldl -lm -lc++ \
      -framework Security -framework CoreFoundation \
      -framework Metal -framework Foundation -framework Accelerate \
      -o "$OUT/aurum_job_cleanup"
    ;;
  Linux)
    cc -std=c11 -I "$INC" "$EX" "$LIB" \
      -lpthread -ldl -lm -lstdc++ \
      -o "$OUT/aurum_job_cleanup"
    ;;
  *)
    echo "host $(uname -s): compile step skipped (layout checks passed)"
    echo "OK qualify_native_sdk_bundle (layout-only host)"
    exit 0
    ;;
esac

# Ensure binary does not DT_NEEDED workspace (best-effort: no -L target in link line)
echo "== run C11 example =="
"$OUT/aurum_job_cleanup" | tee "$WORKDIR/c11.out"
grep -q 'abi=2' "$WORKDIR/c11.out"
grep -q 'cleanup=1\|cleaned:' "$WORKDIR/c11.out" || grep -qi cleaned "$WORKDIR/c11.out"

# C++17 optional
if command -v c++ >/dev/null 2>&1; then
  echo "== compile C++17 engine_raii =="
  case "$(uname -s)" in
    Darwin)
      c++ -std=c++17 -I "$INC" "$DIR/examples/engine_raii.cpp" "$LIB" \
        -lpthread -ldl -lm -lc++ \
        -framework Security -framework CoreFoundation \
        -framework Metal -framework Foundation -framework Accelerate \
        -o "$OUT/aurum_engine_raii"
      ;;
    Linux)
      c++ -std=c++17 -I "$INC" "$DIR/examples/engine_raii.cpp" "$LIB" \
        -lpthread -ldl -lm -lstdc++ \
        -o "$OUT/aurum_engine_raii"
      ;;
  esac
  "$OUT/aurum_engine_raii" | tee "$WORKDIR/cxx.out"
  grep -q 'abi=2' "$WORKDIR/cxx.out"
fi

# Negative: header must not advertise remote provider execution APIs
if grep -Eiq 'openrouter|openai|elevenlabs|remote_provider' "$DIR/include/aurum.h"; then
  echo "FAIL header mentions remote provider surface" >&2
  exit 1
fi

echo "OK qualify_native_sdk_bundle"
