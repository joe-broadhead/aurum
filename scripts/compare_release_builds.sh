#!/usr/bin/env bash
# Two-builder digest comparison for Tier A release binaries (JOE-1885).
#
# Builds the same source twice into isolated target dirs (or compares an
# existing "builder A" binary against a fresh "builder B" rebuild), then writes
# a variance report.
#
# Usage (from repo root):
#   ./scripts/compare_release_builds.sh [--target TRIPLE] [--out DIR]
#   ./scripts/compare_release_builds.sh --baseline PATH/TO/BIN --target TRIPLE
#
# Environment:
#   AURUM_REPRO_STRICT=1  — exit 1 when digests differ (default: report only)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGET=""
OUT_DIR="${ROOT}/dist/repro"
BASELINE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --target) TARGET="$2"; shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    --baseline) BASELINE="$2"; shift 2 ;;
    -h|--help)
      sed -n '1,20p' "$0"
      exit 0
      ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [ -z "${TARGET}" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) TARGET="aarch64-apple-darwin" ;;
    Linux-x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
    MINGW*|MSYS*|CYGWIN*) TARGET="x86_64-pc-windows-msvc" ;;
    *)
      echo "pass --target <rust triple>" >&2
      exit 2
      ;;
  esac
fi

BIN_NAME="aurum"
case "${TARGET}" in
  *windows*) BIN_NAME="aurum.exe" ;;
esac

mkdir -p "${OUT_DIR}"
REPORT="${OUT_DIR}/VARIANCE_REPORT.md"
A_DIR="${OUT_DIR}/builder-a"
B_DIR="${OUT_DIR}/builder-b"
rm -rf "${A_DIR}" "${B_DIR}"
mkdir -p "${A_DIR}" "${B_DIR}"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

build_once() {
  local label="$1"
  local cargo_target="$2"
  local dest="$3"
  echo "== builder ${label}: cargo build --release --target ${TARGET} =="
  CARGO_TARGET_DIR="${cargo_target}" \
    cargo build -p aurum-stt --release --locked --target "${TARGET}"
  local src="${cargo_target}/${TARGET}/release/${BIN_NAME}"
  if [ ! -f "${src}" ]; then
    # package may be named aurum (CLI package)
    src="${cargo_target}/${TARGET}/release/aurum"
    if [[ "${BIN_NAME}" == *.exe ]]; then
      src="${src}.exe"
    fi
  fi
  if [ ! -f "${src}" ]; then
    # Find binary
    src="$(find "${cargo_target}/${TARGET}/release" -maxdepth 1 -type f \( -name 'aurum' -o -name 'aurum.exe' \) | head -1)"
  fi
  if [ -z "${src}" ] || [ ! -f "${src}" ]; then
    echo "build product missing under ${cargo_target}/${TARGET}/release" >&2
    ls -la "${cargo_target}/${TARGET}/release" >&2 || true
    exit 1
  fi
  cp "${src}" "${dest}"
  chmod +x "${dest}" 2>/dev/null || true
}

A_BIN="${A_DIR}/${BIN_NAME}"
B_BIN="${B_DIR}/${BIN_NAME}"

if [ -n "${BASELINE}" ]; then
  if [ ! -f "${BASELINE}" ]; then
    echo "baseline binary not found: ${BASELINE}" >&2
    exit 1
  fi
  cp "${BASELINE}" "${A_BIN}"
  chmod +x "${A_BIN}" 2>/dev/null || true
else
  build_once "A" "${A_DIR}/target" "${A_BIN}"
fi

build_once "B" "${B_DIR}/target" "${B_BIN}"

A_HASH="$(sha256_file "${A_BIN}")"
B_HASH="$(sha256_file "${B_BIN}")"
A_SIZE="$(wc -c < "${A_BIN}" | tr -d ' ')"
B_SIZE="$(wc -c < "${B_BIN}" | tr -d ' ')"
MATCH="no"
if [ "${A_HASH}" = "${B_HASH}" ]; then
  MATCH="yes"
fi

{
  echo "# Build variance report (JOE-1885)"
  echo
  echo "- **generated_at_utc:** $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- **source_commit:** $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "- **target:** \`${TARGET}\`"
  echo "- **package:** \`aurum-stt\` (CLI binary \`${BIN_NAME}\`)"
  echo
  echo "## Digests"
  echo
  echo "| Builder | Path | Bytes | SHA-256 |"
  echo "|---------|------|------:|---------|"
  echo "| A | \`${A_BIN}\` | ${A_SIZE} | \`${A_HASH}\` |"
  echo "| B | \`${B_BIN}\` | ${B_SIZE} | \`${B_HASH}\` |"
  echo
  echo "**Byte-identical:** ${MATCH}"
  echo
  echo "## Expected variance"
  echo
  echo "Rust release binaries are **not** guaranteed bit-reproducible across hosts."
  echo "Known benign differences include:"
  echo
  echo "- Absolute path embeds in debug metadata / panic locations when not fully stripped"
  echo "- Build-id / LC_UUID / PE timestamp fields that bake wall-clock or host entropy"
  echo "- Different LLVM/clang toolchains or linker versions between runner images"
  echo "- macOS ad-hoc code signature blobs when the host re-signs"
  echo
  echo "Hard failures (treat as bugs, not variance):"
  echo
  echo "- Missing binary or size ratio outside 0.5×–2.0×"
  echo "- CLI \`--version\` mismatch between builders"
  echo "- Different behavior on \`doctor --offline\` smoke"
  echo
  if [ "${MATCH}" = "yes" ]; then
    echo "Builders A and B produced **matching** digests for this target."
  else
    echo "Builders A and B **differ**. Compare with \`cmp -l\` / \`llvm-objdump\` if investigating."
    echo "This is recorded as measured variance, not an automatic gate failure"
    echo "(set \`AURUM_REPRO_STRICT=1\` to fail the script on mismatch)."
  fi
} > "${REPORT}"

echo "Wrote ${REPORT}"
cat "${REPORT}"

# Hard size sanity (always fail-closed)
python3 - <<PY
a, b = int("${A_SIZE}"), int("${B_SIZE}")
if a <= 0 or b <= 0:
    raise SystemExit("empty binary")
ratio = max(a, b) / min(a, b)
if ratio > 2.0:
    raise SystemExit(f"size ratio {ratio:.2f} exceeds 2.0x — hard mismatch")
print(f"size ratio OK ({ratio:.3f})")
PY

if [ -x "${A_BIN}" ]; then
  "${A_BIN}" --version || true
fi
if [ -x "${B_BIN}" ]; then
  "${B_BIN}" --version || true
fi

if [ "${MATCH}" != "yes" ] && [ "${AURUM_REPRO_STRICT:-0}" = "1" ]; then
  echo "AURUM_REPRO_STRICT=1 and digests differ" >&2
  exit 1
fi

echo "compare_release_builds OK (match=${MATCH})"
