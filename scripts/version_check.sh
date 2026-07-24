#!/usr/bin/env bash
# Ensure VERSION, workspace Cargo.toml, crate manifests, and CHANGELOG agree.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

version="$(tr -d '[:space:]' < VERSION)"
if ! [[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "VERSION must be semver x.y.z (got: ${version})"
  exit 1
fi

ws_version="$(awk -F'"' '/^version = "/ { print $2; exit }' Cargo.toml)"
if [ "${ws_version}" != "${version}" ]; then
  echo "Cargo.toml workspace version (${ws_version}) != VERSION (${version})"
  exit 1
fi

for manifest in crates/aurum-core/Cargo.toml crates/aurum/Cargo.toml crates/aurum-ffi/Cargo.toml; do
  if grep -q '^version\.workspace = true' "${manifest}"; then
    continue
  fi
  crate_version="$(awk -F'"' '/^version = "/ { print $2; exit }' "${manifest}")"
  if [ -n "${crate_version}" ] && [ "${crate_version}" != "${version}" ]; then
    echo "${manifest} version (${crate_version}) != VERSION (${version})"
    exit 1
  fi
done

if ! grep -qE "^## \\[${version}\\]" CHANGELOG.md; then
  echo "CHANGELOG.md missing ## [${version}] entry"
  exit 1
fi

echo "Version ${version} is consistent."
