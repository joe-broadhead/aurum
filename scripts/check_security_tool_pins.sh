#!/usr/bin/env bash
# JOE-1715: reject permissive cargo-audit / cargo-deny install fallbacks.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0
# Flag any install of cargo-audit/deny that uses `|| cargo install` without an exact --version,
# or that installs without --version at all.
while IFS= read -r file; do
  if grep -nE 'cargo install cargo-(audit|deny)[^\n]*\|\|[^\n]*cargo install cargo-(audit|deny)' "$file" >/dev/null 2>&1; then
    echo "FAIL: permissive fallback install in $file" >&2
    grep -nE 'cargo install cargo-(audit|deny)' "$file" >&2 || true
    fail=1
  fi
  # Require every cargo-audit / cargo-deny install line to pin --version
  while IFS= read -r line; do
    if [[ "$line" == *"cargo install cargo-audit"* || "$line" == *"cargo install cargo-deny"* ]]; then
      if [[ "$line" != *"--version"* ]]; then
        echo "FAIL: unpinned install in $file: $line" >&2
        fail=1
      fi
    fi
  done < <(grep -n 'cargo install cargo-audit\|cargo install cargo-deny' "$file" || true)
done < <(find .github/workflows -type f \( -name '*.yml' -o -name '*.yaml' \))

if [[ "$fail" -ne 0 ]]; then
  echo "Security tool install policy violated (JOE-1715)." >&2
  exit 1
fi
echo "OK: security tool installs are pinned and fail-closed"
