#!/usr/bin/env bash
# Automated RC dogfood subset (JOE-1897).
#
# Fills dist/rc-dogfood evidence for automated rows; manual rows left open.
#
# Usage:
#   ./scripts/rc_dogfood_checklist.sh [--tag v0.0.16] [--out dist/rc-dogfood]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TAG=""
OUT="dist/rc-dogfood"
while [ $# -gt 0 ]; do
  case "$1" in
    --tag) TAG="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help) sed -n '1,20p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

mkdir -p "${OUT}"
STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
HOST="$(uname -s)-$(uname -m)"
EVIDENCE="${OUT}/${HOST}-auto.md"

echo "== rc dogfood automated subset (${HOST}) =="

pass_fail() {
  local name="$1"
  shift
  if "$@"; then
    echo "| ${name} | pass |"
    return 0
  else
    echo "| ${name} | fail |"
    return 1
  fi
}

FAILED=0
{
  echo "# RC dogfood evidence (automated)"
  echo
  echo "- platform: ${HOST}"
  echo "- tag: ${TAG:-local-workspace}"
  echo "- operator: automation"
  echo "- date_utc: ${STAMP}"
  echo "- install_method: source (workspace)"
  echo
  echo "| Check | Result |"
  echo "|-------|--------|"
} > "${EVIDENCE}"

run_row() {
  local name="$1"
  shift
  if "$@" >>"${OUT}/log.txt" 2>&1; then
    echo "| ${name} | pass |" >> "${EVIDENCE}"
  else
    echo "| ${name} | fail |" >> "${EVIDENCE}"
    FAILED=1
  fi
}

: > "${OUT}/log.txt"

run_row "freeze_check" ./scripts/rc_freeze_check.sh
run_row "mutation_semantics" cargo test -p aurum-core --test mutation_semantics --no-default-features --locked
run_row "fault_injection" cargo test -p aurum-core --test fault_injection --locked
run_row "ffi_stress" cargo test -p aurum-ffi --lib --no-default-features --locked -- stress
run_row "clean_install_source" ./scripts/clean_install_smoke.sh --from-source

# Optional: verify published tag if requested and gh/cosign available.
if [ -n "${TAG}" ] && command -v gh >/dev/null 2>&1; then
  if command -v cosign >/dev/null 2>&1; then
    run_row "verify_assets_${TAG}" env AURUM_REQUIRE_COSIGN=1 AURUM_VERIFY_NEGATIVE=1 \
      ./scripts/independent_release_verify.sh --tag "${TAG}"
  else
    echo "| verify_assets_${TAG} | skipped (no cosign) |" >> "${EVIDENCE}"
  fi
else
  echo "| verify_assets | skipped (no --tag or gh) |" >> "${EVIDENCE}"
fi

{
  echo
  echo "## Manual rows (operator)"
  echo
  echo "| Check | Result |"
  echo "|-------|--------|"
  echo "| cold_stt | |"
  echo "| warm_stt | |"
  echo "| offline_stt | |"
  echo "| tts | |"
  echo "| remote | |"
  echo "| upgrade | |"
  echo
  echo "## Notes"
  echo
  echo "- See docs/operations/rc-dogfood.md for full matrix."
  echo "- Log: ${OUT}/log.txt"
  if [ "${FAILED}" -ne 0 ]; then
    echo "- **automated_result: FAIL**"
  else
    echo "- **automated_result: PASS**"
  fi
} >> "${EVIDENCE}"

echo "Wrote ${EVIDENCE}"
cat "${EVIDENCE}"

if [ "${FAILED}" -ne 0 ]; then
  echo "rc_dogfood_checklist: automated rows failed" >&2
  exit 1
fi
echo "rc_dogfood_checklist.sh OK"
