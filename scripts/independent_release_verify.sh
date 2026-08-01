#!/usr/bin/env bash
# Independent clean-environment release verification (JOE-1891).
#
# Downloads a published GitHub Release into a temp dir and runs
# verify_release_assets.sh with cosign required. Intended for a runner that did
# not build the assets (separate from release.yml publish job).
#
# Usage:
#   ./scripts/independent_release_verify.sh [--tag v0.0.14] [--repo owner/name]
#
# Env:
#   AURUM_RELEASE_TAG   — default: latest non-draft release tag via gh
#   AURUM_GITHUB_REPO   — default: joe-broadhead/aurum
#   AURUM_VERIFY_NEGATIVE — if 1, also run a negative checksum mutation check
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

REPO="${AURUM_GITHUB_REPO:-joe-broadhead/aurum}"
TAG="${AURUM_RELEASE_TAG:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --tag) TAG="$2"; shift 2 ;;
    --repo) REPO="$2"; shift 2 ;;
    -h|--help) sed -n '1,25p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI required" >&2
  exit 1
fi

if [ -z "${TAG}" ]; then
  TAG="$(gh release list --repo "${REPO}" --limit 20 --json tagName,isDraft,isPrerelease \
    --jq '[.[] | select(.isDraft==false)][0].tagName')"
fi
if [ -z "${TAG}" ] || [ "${TAG}" = "null" ]; then
  echo "could not resolve release tag" >&2
  exit 1
fi

echo "== independent verify tag=${TAG} repo=${REPO} =="

TMP="$(mktemp -d "${TMPDIR:-/tmp}/aurum-ind-verify-XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT
ASSETS="${TMP}/assets"
mkdir -p "${ASSETS}"

gh release download "${TAG}" --repo "${REPO}" --dir "${ASSETS}"
ls -la "${ASSETS}"

# Resolve commit for PROVENANCE expectations when possible.
COMMIT="$(gh api "repos/${REPO}/git/ref/tags/${TAG}" --jq '.object.sha' 2>/dev/null || true)"
# Annotated tags point at a tag object; resolve to commit.
if [ -n "${COMMIT}" ]; then
  OBJ_TYPE="$(gh api "repos/${REPO}/git/ref/tags/${TAG}" --jq '.object.type' 2>/dev/null || echo commit)"
  if [ "${OBJ_TYPE}" = "tag" ]; then
    COMMIT="$(gh api "repos/${REPO}/git/tags/${COMMIT}" --jq '.object.sha' 2>/dev/null || echo "${COMMIT}")"
  fi
fi

if ! command -v cosign >/dev/null 2>&1; then
  echo "Installing cosign via official installer is expected in CI; local: install cosign."
  # Best-effort: use go install or skip hard require only if AURUM_REQUIRE_COSIGN=0
fi

export AURUM_EXPECT_TAG="${TAG}"
if [ -n "${COMMIT}" ]; then
  export AURUM_EXPECT_COMMIT="${COMMIT}"
fi
export AURUM_REQUIRE_COSIGN="${AURUM_REQUIRE_COSIGN:-1}"
export AURUM_COSIGN_CERTIFICATE_OIDC_ISSUER="${AURUM_COSIGN_CERTIFICATE_OIDC_ISSUER:-https://token.actions.githubusercontent.com}"
export AURUM_COSIGN_CERTIFICATE_IDENTITY="${AURUM_COSIGN_CERTIFICATE_IDENTITY:-https://github.com/${REPO}/.github/workflows/release.yml@refs/tags/${TAG}}"

chmod +x "${ROOT}/scripts/verify_release_assets.sh"
"${ROOT}/scripts/verify_release_assets.sh" "${ASSETS}"

# Negative path: corrupt one byte of a binary and ensure checksum check fails.
if [ "${AURUM_VERIFY_NEGATIVE:-1}" = "1" ]; then
  echo "== negative path: mutated asset must fail checksum =="
  NEG="${TMP}/neg"
  mkdir -p "${NEG}"
  cp -a "${ASSETS}/." "${NEG}/"
  target="$(find "${NEG}" -maxdepth 1 -type f \( -name 'aurum-*' -o -name 'aurum*.exe' \) | head -1)"
  if [ -n "${target}" ] && [ -f "${target}" ]; then
    # Flip a mid-file byte without rewriting the whole file.
    python3 - <<PY
from pathlib import Path
p = Path("${target}")
data = bytearray(p.read_bytes())
if not data:
    raise SystemExit("empty target")
i = min(len(data) // 2, len(data) - 1)
data[i] ^= 0xFF
p.write_bytes(data)
print(f"mutated {p.name} at offset {i}")
PY
    set +e
    AURUM_REQUIRE_COSIGN=0 "${ROOT}/scripts/verify_release_assets.sh" "${NEG}"
    rc=$?
    set -e
    if [ "${rc}" -eq 0 ]; then
      echo "negative path unexpectedly succeeded" >&2
      exit 1
    fi
    echo "negative path correctly failed (rc=${rc})"
  else
    echo "no binary found for negative path; skip"
  fi
fi

echo "independent_release_verify.sh OK (${TAG})"
