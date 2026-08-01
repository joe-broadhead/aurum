#!/usr/bin/env bash
# Clean-install qualification smoke (JOE-1863).
#
# Installs Aurum into a temporary directory, runs offline doctor, optionally
# verifies a release download, then uninstalls the binary.
#
# Usage:
#   ./scripts/clean_install_smoke.sh --from-source
#   ./scripts/clean_install_smoke.sh --from-release --version v0.0.11
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="from-source"
VERSION="${AURUM_VERSION:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --from-source) MODE="from-source"; shift ;;
    --from-release) MODE="from-release"; shift ;;
    --version) VERSION="$2"; shift 2 ;;
    -h|--help)
      sed -n '1,20p' "$0"
      exit 0
      ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

TMP="$(mktemp -d "${TMPDIR:-/tmp}/aurum-clean-XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT

export AURUM_INSTALL_DIR="${TMP}/bin"
export AURUM_CACHE_DIR="${TMP}/cache"
mkdir -p "${AURUM_INSTALL_DIR}" "${AURUM_CACHE_DIR}"

echo "== clean install smoke (${MODE}) → ${TMP} =="

if [ "${MODE}" = "from-release" ]; then
  args=(--from-release)
  if [ -n "${VERSION}" ]; then
    export AURUM_VERSION="${VERSION}"
    args+=(--version "${VERSION}")
  fi
  ./scripts/install.sh "${args[@]}"
else
  ./scripts/install.sh --from-source
fi

AURUM_BIN="${AURUM_INSTALL_DIR}/aurum"
if [ ! -x "${AURUM_BIN}" ] && [ -x "${AURUM_INSTALL_DIR}/aurum.exe" ]; then
  AURUM_BIN="${AURUM_INSTALL_DIR}/aurum.exe"
fi
if [ ! -x "${AURUM_BIN}" ]; then
  echo "install did not produce executable at ${AURUM_INSTALL_DIR}" >&2
  ls -la "${AURUM_INSTALL_DIR}" >&2 || true
  exit 1
fi

echo "== aurum --version =="
"${AURUM_BIN}" --version

echo "== aurum doctor (offline) =="
# Prefer offline doctor flags if present; otherwise plain doctor.
if "${AURUM_BIN}" doctor --help 2>&1 | grep -q offline; then
  "${AURUM_BIN}" doctor --offline || "${AURUM_BIN}" doctor
else
  "${AURUM_BIN}" doctor
fi

echo "== uninstall binary =="
./scripts/install.sh --uninstall || rm -f "${AURUM_BIN}"

if [ -x "${AURUM_BIN}" ]; then
  echo "binary still present after uninstall" >&2
  exit 1
fi

echo "clean_install_smoke OK (${MODE})"
