#!/usr/bin/env bash
# Package a Tier A native SDK archive from a release aurum-ffi build (JOE-2225).
#
# Usage:
#   ./scripts/package_native_sdk.sh [--features none|default] [--out-dir dist/native-sdk]
#
# Produces: dist/native-sdk/aurum-sdk-<version>-<triple>.tar.gz (+ .sha256)
# Layout is deterministic and free of Cargo target-directory noise.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

FEATURES="none"
OUT_DIR="${ROOT}/dist/native-sdk"
abs_path() {
  # Make absolute on Unix and Windows Git-Bash (avoid /tmp Python path issues).
  local p="$1"
  if [[ "${p}" = /* || "${p}" =~ ^[A-Za-z]:[\\/] ]]; then
    printf '%s\n' "${p}"
  else
    printf '%s\n' "${ROOT}/${p}"
  fi
}
while [[ $# -gt 0 ]]; do
  case "$1" in
    --features) FEATURES="$2"; shift 2 ;;
    --out-dir)
      OUT_DIR="$(abs_path "$2")"
      shift 2
      ;;
    -h|--help)
      echo "Usage: $0 [--features none|default] [--out-dir DIR]"
      exit 0
      ;;
    *) echo "unknown: $1" >&2; exit 2 ;;
  esac
done

VERSION="$(tr -d '[:space:]' < VERSION)"
case "$(uname -s)" in
  Darwin) OS=macos; ARCH="$(uname -m)"; [[ "$ARCH" == "arm64" ]] && TRIPLE="aarch64-apple-darwin" || TRIPLE="x86_64-apple-darwin" ;;
  Linux) OS=linux; ARCH="$(uname -m)"; [[ "$ARCH" == "x86_64" ]] && TRIPLE="x86_64-unknown-linux-gnu" || TRIPLE="${ARCH}-unknown-linux-gnu" ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT) OS=windows; TRIPLE="x86_64-pc-windows-msvc" ;;
  *) echo "unsupported host $(uname -s)" >&2; exit 1 ;;
esac

CARGO_FEATURES=(--no-default-features)
BUILD_FEATURES="stt_cleanup_only"
if [[ "$FEATURES" == "default" ]]; then
  CARGO_FEATURES=()
  BUILD_FEATURES="default"
fi

echo "== build aurum-ffi release ($BUILD_FEATURES) =="
cargo build -p aurum-ffi --release --locked "${CARGO_FEATURES[@]}"

# Stage under the repo (not /tmp): Windows GHA bash + system Python mishandle /tmp paths.
mkdir -p "${ROOT}/dist"
STAGE="$(mktemp -d "${ROOT}/dist/.sdk-stage-XXXXXX")"
# Resolve to a real absolute path for both bash and Python.
STAGE="$(cd "${STAGE}" && pwd -P 2>/dev/null || pwd)"
trap 'rm -rf "$STAGE"' EXIT
PREFIX="${STAGE}/aurum-sdk-${VERSION}-${TRIPLE}"
mkdir -p "${PREFIX}/include" "${PREFIX}/lib" "${PREFIX}/cmake" "${PREFIX}/pkg-config" \
  "${PREFIX}/examples" "${PREFIX}/share/doc"

cp crates/aurum-ffi/include/aurum.h "${PREFIX}/include/"
cp crates/aurum-ffi/examples/job_cleanup.c "${PREFIX}/examples/"
cp crates/aurum-ffi/examples/engine_raii.cpp "${PREFIX}/examples/"
cp crates/aurum-ffi/LICENSE "${PREFIX}/share/doc/LICENSE" 2>/dev/null || cp LICENSE "${PREFIX}/share/doc/LICENSE"
cp crates/aurum-ffi/README.md "${PREFIX}/share/doc/README-ffi.md"

# Libraries
STATIC_LIB="libaurum_ffi.a"
if [[ "$OS" == "windows" ]]; then
  STATIC_LIB="aurum_ffi.lib"
  # MSVC may place import/static libs under target/release or with deps.
  shopt -s nullglob
  for f in \
    target/release/aurum_ffi.lib \
    target/release/aurum_ffi.dll \
    target/release/aurum_ffi.dll.lib \
    target/release/deps/aurum_ffi*.lib \
    target/release/deps/aurum_ffi*.dll
  do
    [[ -f "$f" ]] || continue
    cp "$f" "${PREFIX}/lib/"
  done
  shopt -u nullglob
  if ! ls "${PREFIX}/lib"/*.{lib,dll} >/dev/null 2>&1; then
    echo "Windows package: no aurum_ffi .lib/.dll found under target/release" >&2
    ls -la target/release 2>/dev/null | head -40 || true
    exit 1
  fi
else
  cp "target/release/${STATIC_LIB}" "${PREFIX}/lib/"
  if [[ "$OS" == "macos" && -f target/release/libaurum_ffi.dylib ]]; then
    cp target/release/libaurum_ffi.dylib "${PREFIX}/lib/" || true
  fi
  if [[ "$OS" == "linux" && -f target/release/libaurum_ffi.so ]]; then
    cp target/release/libaurum_ffi.so "${PREFIX}/lib/" || true
  fi
fi

# CMake package for all Tier A platforms (includes system link deps — v0.0.23 B).
cp native/sdk/cmake/AurumConfig.cmake.in "${PREFIX}/cmake/AurumConfig.cmake.in"
cp native/sdk/examples/CMakeLists.txt "${PREFIX}/examples/CMakeLists.txt" 2>/dev/null || true
# Portable imported target without configure_package_config_file.
# Path-relative to this file's parent (SDK root).
cat > "${PREFIX}/cmake/AurumConfig.cmake" <<EOF
# Aurum native SDK (JOE-2225 / v0.0.23 B) — path-relative to this file's ..
get_filename_component(AURUM_SDK_ROOT "\${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
set(AURUM_VERSION "${VERSION}")
set(AURUM_ABI_VERSION 2)
set(AURUM_ABI_MIN_VERSION 2)
set(AURUM_INCLUDE_DIR "\${AURUM_SDK_ROOT}/include")
set(AURUM_LIB_DIR "\${AURUM_SDK_ROOT}/lib")
if(NOT TARGET Aurum::aurum_ffi)
  add_library(Aurum::aurum_ffi STATIC IMPORTED)
  set_target_properties(Aurum::aurum_ffi PROPERTIES
    IMPORTED_LOCATION "\${AURUM_LIB_DIR}/${STATIC_LIB}"
    INTERFACE_INCLUDE_DIRECTORIES "\${AURUM_INCLUDE_DIR}"
  )
  if(APPLE)
    target_link_libraries(Aurum::aurum_ffi INTERFACE
      "-framework Security" "-framework CoreFoundation"
      "-framework Metal" "-framework Foundation" "-framework Accelerate"
      pthread dl m c++)
  elseif(WIN32)
    target_link_libraries(Aurum::aurum_ffi INTERFACE
      ws2_32 userenv ntdll bcrypt advapi32 kernel32 shell32 ole32 uuid
      synchronization)
  elseif(UNIX)
    target_link_libraries(Aurum::aurum_ffi INTERFACE pthread dl m stdc++)
  endif()
endif()
EOF

# pkg-config (Unix); Windows consumers use CMake / MSVC flags from BUILD.md
if [[ "$OS" != "windows" ]]; then
  if [[ "$OS" == "macos" ]]; then
    LIBS_PRIVATE="-lpthread -ldl -lm -lc++ -framework Security -framework CoreFoundation -framework Metal -framework Foundation -framework Accelerate"
  else
    LIBS_PRIVATE="-lpthread -ldl -lm -lstdc++"
  fi
  sed -e "s|@PREFIX@|/usr/local|g" \
      -e "s|@AURUM_VERSION@|${VERSION}|g" \
      -e "s|@LIBS_PRIVATE@|${LIBS_PRIVATE}|g" \
      native/sdk/pkg-config/aurum.pc.in > "${PREFIX}/pkg-config/aurum.pc"
fi

# Build instructions
cat > "${PREFIX}/BUILD.md" <<EOF
# Building examples from this SDK (JOE-2225 / v0.0.23 B)

Version: ${VERSION}
Triple: ${TRIPLE}
Features: ${BUILD_FEATURES}
ABI: 2 (min 2)

## Direct compiler (staticlib)

### macOS
\`\`\`bash
cc -std=c11 -I include examples/job_cleanup.c lib/${STATIC_LIB} \\
  -lpthread -ldl -lm -lc++ \\
  -framework Security -framework CoreFoundation \\
  -framework Metal -framework Foundation -framework Accelerate \\
  -o /tmp/aurum_job_cleanup
\`\`\`

### Linux
\`\`\`bash
cc -std=c11 -I include examples/job_cleanup.c lib/${STATIC_LIB} \\
  -lpthread -ldl -lm -lstdc++ -o /tmp/aurum_job_cleanup
\`\`\`

### Windows (MSVC, x64 Developer Command Prompt)
\`\`\`bat
cl /std:c11 /I include examples\\job_cleanup.c /link lib\\${STATIC_LIB} ^
  ws2_32.lib userenv.lib ntdll.lib bcrypt.lib advapi32.lib shell32.lib ole32.lib uuid.lib ^
  /OUT:aurum_job_cleanup.exe
\`\`\`

C++17 RAII example: compile \`examples/engine_raii.cpp\` with the same libs.

## CMake
\`\`\`bash
cmake -S examples -B /tmp/aurum-sdk-build -DAURUM_SDK_ROOT="\$PWD"
cmake --build /tmp/aurum-sdk-build
\`\`\`

Or:
\`\`\`cmake
include(\${SDK}/cmake/AurumConfig.cmake)
target_link_libraries(app PRIVATE Aurum::aurum_ffi)
\`\`\`

## pkg-config (Unix)
\`\`\`bash
export PKG_CONFIG_PATH="\$PWD/pkg-config:\${PKG_CONFIG_PATH:-}"
# aurum.pc uses prefix=/usr/local by default — override for local extract:
#   pkg-config --define-variable=prefix=\$PWD --libs --cflags aurum
\`\`\`

## Runtime
- STT PCM is mono f32 @ 16000 Hz.
- Models/packs are **not** in the archive; first-run verified download uses the engine cache.
- Remote providers are **not** available through the C ABI.
- Do not place untrusted directories on the dynamic-library search path.
EOF

# SDK manifest (deterministic field order via python; env paths for Windows).
PREFIX_ENV="${PREFIX}" VERSION_ENV="${VERSION}" TRIPLE_ENV="${TRIPLE}" \
BUILD_FEATURES_ENV="${BUILD_FEATURES}" \
python3 - <<'PY'
import hashlib, json, os, platform
from pathlib import Path
prefix = Path(os.environ["PREFIX_ENV"]).resolve()
assert prefix.is_dir(), prefix
files = {}
for p in sorted(prefix.rglob("*")):
    if p.is_file():
        rel = p.relative_to(prefix).as_posix()
        h = hashlib.sha256(p.read_bytes()).hexdigest()
        files[rel] = {"sha256": h, "size": p.stat().st_size}
manifest = {
    "schema_version": 1,
    "aurum_version": os.environ["VERSION_ENV"],
    "target_triple": os.environ["TRIPLE_ENV"],
    "abi_version": 2,
    "abi_min_version": 2,
    "build_features": os.environ["BUILD_FEATURES_ENV"],
    "remote_via_c_abi": False,
    "host": {
        "os": platform.system(),
        "arch": platform.machine(),
    },
    "files": files,
    "notes": [
        "No FFmpeg bundled",
        "Models not included",
        "Remote providers unsupported through C ABI",
    ],
}
(prefix / "SDK_MANIFEST.json").write_text(
    json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
print("manifest files:", len(files), "at", prefix)
PY

# Symbol allowlist copy
cp native/sdk/symbols-allowlist.txt "${PREFIX}/share/doc/symbols-allowlist.txt"

mkdir -p "$OUT_DIR"
ARCHIVE_NAME="aurum-sdk-${VERSION}-${TRIPLE}.tar.gz"
(
  cd "$STAGE"
  # Deterministic-ish tar: sorted names, owner 0
  tar --uid=0 --gid=0 --numeric-owner -czf "${OUT_DIR}/${ARCHIVE_NAME}" \
    "aurum-sdk-${VERSION}-${TRIPLE}" 2>/dev/null \
    || tar -czf "${OUT_DIR}/${ARCHIVE_NAME}" "aurum-sdk-${VERSION}-${TRIPLE}"
)

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$OUT_DIR" && sha256sum "${ARCHIVE_NAME}" > "${ARCHIVE_NAME}.sha256")
else
  (cd "$OUT_DIR" && shasum -a 256 "${ARCHIVE_NAME}" > "${ARCHIVE_NAME}.sha256")
fi

echo "Wrote ${OUT_DIR}/${ARCHIVE_NAME}"
cat "${OUT_DIR}/${ARCHIVE_NAME}.sha256"
