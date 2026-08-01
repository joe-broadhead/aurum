#!/usr/bin/env bash
# Publish missing crates.io versions from git tags (local or CI).
#
# Env:
#   VERSIONS   — must be a subset of the code-owned ALLOWLIST (see below)
#   CRATES     — must be a subset of CRATE_ORDER (aurum-core,aurum-stt,aurum-ffi)
#   DRY_RUN    — true|false (default true)
#   CARGO_REGISTRY_TOKEN — emergency token path only (JOE-1715); required when DRY_RUN=false
#   SKIP_IF_EXISTS — true (default): skip crate/version already on crates.io
#   SLEEP_SECS — pause between publishes (default 45)
#
# Usage (from a full-fetch clone of aurum):
#   DRY_RUN=true ./scripts/publish_crates_backfill.sh
#   DRY_RUN=false CARGO_REGISTRY_TOKEN=... ./scripts/publish_crates_backfill.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Reviewed allowlist of historical tags eligible for backfill (JOE-1915 / JOE-1920).
# VERSIONS env may only request a subset of this list.
ALLOWLIST_VERSIONS="0.0.10,0.0.11,0.0.12,0.0.13,0.0.14,0.0.15,0.0.16,0.0.17"
VERSIONS="${VERSIONS:-${ALLOWLIST_VERSIONS}}"
CRATES="${CRATES:-aurum-core,aurum-stt,aurum-ffi}"
DRY_RUN="${DRY_RUN:-true}"
SKIP_IF_EXISTS="${SKIP_IF_EXISTS:-true}"
SLEEP_SECS="${SLEEP_SECS:-45}"

# Reject versions not on the allowlist.
IFS=',' read -r -a REQ_VERS <<< "${VERSIONS}"
IFS=',' read -r -a ALLOW_VERS <<< "${ALLOWLIST_VERSIONS}"
for ver in "${REQ_VERS[@]}"; do
  ver="$(echo "${ver}" | tr -d '[:space:]')"
  [ -n "${ver}" ] || continue
  ok=0
  for a in "${ALLOW_VERS[@]}"; do
    a="$(echo "${a}" | tr -d '[:space:]')"
    if [ "${ver}" = "${a}" ]; then ok=1; break; fi
  done
  if [ "${ok}" -ne 1 ]; then
    echo "ERROR: version ${ver} is not on ALLOWLIST_VERSIONS" >&2
    exit 1
  fi
done

if [ "${DRY_RUN}" != "true" ] && [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
  echo "CARGO_REGISTRY_TOKEN required when DRY_RUN=false (emergency token path; JOE-1715)" >&2
  exit 1
fi

crate_has_version() {
  local crate="$1" ver="$2"
  curl -sL -A "aurum-backfill/1.0" "https://crates.io/api/v1/crates/${crate}/${ver}" \
    | python3 -c "import sys,json; d=json.load(sys.stdin); sys.exit(0 if 'version' in d else 1)" 2>/dev/null
}

publish_one() {
  local crate="$1"
  # JOE-1915 / F-002: never --no-verify; cargo re-checks the exact package.
  local args=(-p "${crate}" --locked)
  if [ "${DRY_RUN}" = "true" ]; then
    args+=(--dry-run)
  fi
  echo ">> cargo publish ${args[*]}"
  cargo publish "${args[@]}"
}

ORIG_REF="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || git rev-parse HEAD)"
cleanup() {
  git checkout -q "${ORIG_REF}" 2>/dev/null || true
}
trap cleanup EXIT

IFS=',' read -r -a VER_ARR <<< "${VERSIONS}"
IFS=',' read -r -a CRATE_ARR <<< "${CRATES}"

for ver in "${VER_ARR[@]}"; do
  ver="$(echo "${ver}" | tr -d '[:space:]')"
  [ -n "${ver}" ] || continue
  tag="v${ver}"
  echo
  echo "========== ${tag} =========="
  if ! git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    echo "ERROR: missing tag ${tag}" >&2
    exit 1
  fi
  git checkout -q --detach "${tag}"
  file_ver="$(tr -d '[:space:]' < VERSION)"
  if [ "${file_ver}" != "${ver}" ]; then
    echo "ERROR: VERSION (${file_ver}) != ${ver} at ${tag}" >&2
    exit 1
  fi
  if [ -x scripts/version_check.sh ]; then
    # Fail closed on version/package drift (JOE-1915).
    ./scripts/version_check.sh
  else
    echo "ERROR: scripts/version_check.sh missing or not executable" >&2
    exit 1
  fi

  for crate in "${CRATE_ARR[@]}"; do
    crate="$(echo "${crate}" | tr -d '[:space:]')"
    [ -n "${crate}" ] || continue
    if [ "${SKIP_IF_EXISTS}" = "true" ] && [ "${DRY_RUN}" != "true" ]; then
      if crate_has_version "${crate}" "${ver}"; then
        echo "skip ${crate}@${ver} (already on crates.io)"
        continue
      fi
    fi
    echo "--- publish ${crate}@${ver} (dry_run=${DRY_RUN}) ---"
    publish_one "${crate}"
    if [ "${DRY_RUN}" != "true" ]; then
      # crates.io index / rate-limit breathing room
      echo "sleep ${SLEEP_SECS}s..."
      sleep "${SLEEP_SECS}"
    fi
  done
done

echo
echo "publish_crates_backfill.sh finished (dry_run=${DRY_RUN})"
