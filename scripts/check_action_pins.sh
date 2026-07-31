#!/usr/bin/env bash
# Fail if any third-party GitHub Action uses a mutable tag ref (JOE-1634).
# Allowed: full 40-char commit SHAs. Local actions (./) are OK.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0
while IFS= read -r line; do
  # shellcheck disable=SC2001
  ref="$(echo "$line" | sed -n 's/.*uses:[[:space:]]*\([^[:space:]#]*\).*/\1/p')"
  [[ -z "${ref}" ]] && continue
  # Skip local/composite
  if [[ "${ref}" == ./* ]]; then
    continue
  fi
  # owner/name@ref
  pin="${ref##*@}"
  if [[ "${pin}" =~ ^[0-9a-f]{40}$ ]]; then
    continue
  fi
  echo "UNPINNED ACTION: ${line}"
  fail=1
done < <(grep -REn --include='*.yml' --include='*.yaml' '^\s*uses:' .github/workflows || true)

if [[ "${fail}" -ne 0 ]]; then
  echo "All third-party Actions must be pinned to a full commit SHA (JOE-1634)."
  exit 1
fi
echo "Action pin check OK."
