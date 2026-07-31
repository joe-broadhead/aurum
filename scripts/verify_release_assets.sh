#!/usr/bin/env bash
# Verify a release asset directory against SHA256SUMS (JOE-1635/1636).
set -euo pipefail
DIR="${1:-}"
if [ -z "${DIR}" ] || [ ! -d "${DIR}" ]; then
  echo "usage: $0 <release-assets-dir>"
  exit 2
fi
cd "${DIR}"
if [ ! -f SHA256SUMS ]; then
  echo "SHA256SUMS missing in ${DIR}"
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum -c SHA256SUMS
elif command -v shasum >/dev/null 2>&1; then
  shasum -a 256 -c SHA256SUMS
else
  echo "no sha256 tool available"
  exit 1
fi
echo "All checksums OK."
