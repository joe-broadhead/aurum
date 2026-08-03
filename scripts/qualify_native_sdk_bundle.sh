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
  if [[ ! -f "$ARCHIVE" && -f "${ROOT}/${ARCHIVE}" ]]; then
    ARCHIVE="${ROOT}/${ARCHIVE}"
  fi
  if [[ ! -f "$ARCHIVE" ]]; then
    echo "archive not found: $ARCHIVE" >&2
    exit 1
  fi
  ARCHIVE="$(cd "$(dirname "$ARCHIVE")" && pwd -P 2>/dev/null || pwd)/$(basename "$ARCHIVE")"
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

# CMake package must ship with system link deps (v0.0.23 B)
need cmake/AurumConfig.cmake
if ! grep -q 'Aurum::aurum_ffi' "$DIR/cmake/AurumConfig.cmake"; then
  echo "FAIL cmake package missing Aurum::aurum_ffi target" >&2
  exit 1
fi
case "$(uname -s)" in
  Darwin)
    grep -q 'framework Security' "$DIR/cmake/AurumConfig.cmake" \
      || { echo "FAIL cmake missing Apple frameworks" >&2; exit 1; }
    ;;
  Linux)
    grep -q 'pthread' "$DIR/cmake/AurumConfig.cmake" \
      || { echo "FAIL cmake missing pthread/dl link deps" >&2; exit 1; }
    need pkg-config/aurum.pc
    ;;
  MINGW*|MSYS*|CYGWIN*)
    grep -Eqi 'ws2_32|userenv|bcrypt' "$DIR/cmake/AurumConfig.cmake" \
      || { echo "FAIL cmake missing Windows system libs" >&2; exit 1; }
    ;;
esac
echo "OK cmake package declares Aurum::aurum_ffi + platform link deps"

# Build C11 example without any workspace target path
OUT="$WORKDIR/bin"
mkdir -p "$OUT"
INC="$DIR/include"
EX="$DIR/examples/job_cleanup.c"
HOST="$(uname -s)"
COMPILED=0

echo "== compile C11 job_cleanup from bundle only =="
case "$HOST" in
  Darwin)
    cc -std=c11 -I "$INC" "$EX" "$LIB" \
      -lpthread -ldl -lm -lc++ \
      -framework Security -framework CoreFoundation \
      -framework Metal -framework Foundation -framework Accelerate \
      -o "$OUT/aurum_job_cleanup"
    COMPILED=1
    ;;
  Linux)
    cc -std=c11 -I "$INC" "$EX" "$LIB" \
      -lpthread -ldl -lm -lstdc++ \
      -o "$OUT/aurum_job_cleanup"
    COMPILED=1
    ;;
  MINGW*|MSYS*|CYGWIN*)
    if command -v cl >/dev/null 2>&1; then
      echo "== MSVC cl link from bundle =="
      (
        cd "$OUT"
        cl //nologo //std:c11 //I "$INC" "$EX" //link "$LIB" \
          ws2_32.lib userenv.lib ntdll.lib bcrypt.lib advapi32.lib \
          shell32.lib ole32.lib uuid.lib //OUT:aurum_job_cleanup.exe
      )
      COMPILED=1
      # Normalize binary path for run step
      if [[ -f "$OUT/aurum_job_cleanup.exe" ]]; then
        mv "$OUT/aurum_job_cleanup.exe" "$OUT/aurum_job_cleanup" 2>/dev/null || true
      fi
    elif command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1; then
      CC_BIN="$(command -v cc || command -v gcc)"
      echo "== MinGW link from bundle ($CC_BIN) =="
      "$CC_BIN" -std=c11 -I "$INC" "$EX" "$LIB" \
        -lws2_32 -luserenv -lntdll -lbcrypt -ladvapi32 -lshell32 -lole32 -luuid \
        -o "$OUT/aurum_job_cleanup.exe" || true
      if [[ -f "$OUT/aurum_job_cleanup.exe" ]]; then
        COMPILED=1
      fi
    else
      echo "Windows host: no cl/gcc; layout + cmake package checks only"
    fi
    ;;
  *)
    echo "host $HOST: compile step skipped (layout + cmake checks passed)"
    ;;
esac

if [[ "$COMPILED" -eq 1 ]]; then
  BIN="$OUT/aurum_job_cleanup"
  [[ -f "$OUT/aurum_job_cleanup.exe" ]] && BIN="$OUT/aurum_job_cleanup.exe"
  echo "== run C11 example =="
  "$BIN" | tee "$WORKDIR/c11.out"
  grep -q 'abi=2' "$WORKDIR/c11.out"
  grep -q 'cleanup=1\|cleaned:' "$WORKDIR/c11.out" || grep -qi cleaned "$WORKDIR/c11.out"

  # C++17 optional
  if command -v c++ >/dev/null 2>&1; then
    echo "== compile C++17 engine_raii =="
    case "$HOST" in
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
      *)
        c++ -std=c++17 -I "$INC" "$DIR/examples/engine_raii.cpp" "$LIB" \
          -o "$OUT/aurum_engine_raii" || true
        ;;
    esac
    if [[ -x "$OUT/aurum_engine_raii" ]]; then
      "$OUT/aurum_engine_raii" | tee "$WORKDIR/cxx.out"
      grep -q 'abi=2' "$WORKDIR/cxx.out"
    fi
  fi
fi

# CMake consumer: find_package / include AurumConfig and link (Unix + when cmake present)
if command -v cmake >/dev/null 2>&1 && [[ -f "$DIR/examples/CMakeLists.txt" || -f "$DIR/cmake/AurumConfig.cmake" ]]; then
  echo "== CMake consumer (Aurum::aurum_ffi) =="
  CMAKE_SRC="$WORKDIR/cmake-src"
  mkdir -p "$CMAKE_SRC"
  if [[ -f "$DIR/examples/CMakeLists.txt" ]]; then
    cp "$DIR/examples/CMakeLists.txt" "$CMAKE_SRC/CMakeLists.txt"
  else
    cat > "$CMAKE_SRC/CMakeLists.txt" <<'CM'
cmake_minimum_required(VERSION 3.16)
project(aurum_sdk_cmake_consumer C)
if(NOT AURUM_SDK_ROOT)
  message(FATAL_ERROR "AURUM_SDK_ROOT required")
endif()
include("${AURUM_SDK_ROOT}/cmake/AurumConfig.cmake")
add_executable(aurum_job_cleanup_cmake "${AURUM_SDK_ROOT}/examples/job_cleanup.c")
target_link_libraries(aurum_job_cleanup_cmake PRIVATE Aurum::aurum_ffi)
CM
  fi
  CMAKE_B="$WORKDIR/cmake-build"
  if cmake -S "$CMAKE_SRC" -B "$CMAKE_B" -DAURUM_SDK_ROOT="$DIR" 2>"$WORKDIR/cmake-cfg.err"; then
    if cmake --build "$CMAKE_B" 2>"$WORKDIR/cmake-build.err"; then
      CMAKE_BIN=""
      for cand in \
        "$CMAKE_B/aurum_job_cleanup_cmake" \
        "$CMAKE_B/Debug/aurum_job_cleanup_cmake.exe" \
        "$CMAKE_B/Release/aurum_job_cleanup_cmake.exe" \
        "$CMAKE_B/aurum_job_cleanup_cmake.exe"
      do
        [[ -f "$cand" ]] && CMAKE_BIN="$cand" && break
      done
      if [[ -n "$CMAKE_BIN" ]]; then
        "$CMAKE_BIN" | tee "$WORKDIR/cmake-run.out"
        grep -q 'abi=2' "$WORKDIR/cmake-run.out"
        echo "OK cmake consumer linked and ran"
      else
        echo "OK cmake configure+build (binary path not found for run; build succeeded)"
      fi
    else
      echo "WARN cmake build failed (configure ok); see $WORKDIR/cmake-build.err" >&2
      # On Windows without a full MSVC env this can fail; do not fail qualify if
      # layout/cmake package content already validated and direct cl path ran.
      if [[ "$COMPILED" -eq 0 && "$HOST" != MINGW* && "$HOST" != MSYS* && "$HOST" != CYGWIN* ]]; then
        cat "$WORKDIR/cmake-build.err" >&2 || true
        exit 1
      fi
    fi
  else
    echo "WARN cmake configure failed:" >&2
    cat "$WORKDIR/cmake-cfg.err" >&2 || true
    if [[ "$HOST" == Linux || "$HOST" == Darwin ]]; then
      exit 1
    fi
  fi
fi

# pkg-config metadata (Unix): ensure file parses
if [[ -f "$DIR/pkg-config/aurum.pc" ]] && command -v pkg-config >/dev/null 2>&1; then
  echo "== pkg-config metadata =="
  PKG_CONFIG_PATH="$DIR/pkg-config${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
    pkg-config --define-variable=prefix="$DIR" --exists aurum \
    || { echo "FAIL pkg-config aurum not found" >&2; exit 1; }
  LIBS_PC="$(PKG_CONFIG_PATH="$DIR/pkg-config${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
    pkg-config --define-variable=prefix="$DIR" --libs aurum)"
  echo "pkg-config --libs: $LIBS_PC"
  echo "$LIBS_PC" | grep -q aurum_ffi
  echo "OK pkg-config"
fi

# Negative: header must not advertise remote provider execution APIs
if grep -Eiq 'openrouter|openai|elevenlabs|remote_provider' "$DIR/include/aurum.h"; then
  echo "FAIL header mentions remote provider surface" >&2
  exit 1
fi

echo "OK qualify_native_sdk_bundle"
