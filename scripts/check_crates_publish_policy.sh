#!/usr/bin/env bash
# Static policy checks for crates.io publish workflows (JOE-1915 / F-002).
# Fails if production publish can target mutable refs, skip tests, or --no-verify.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0

must_exist() {
  local path="$1"
  if [ ! -f "${path}" ]; then
    echo "FAIL: missing ${path}"
    fail=1
    return 1
  fi
  return 0
}

# Forbid pattern in file
forbid() {
  local desc="$1" path="$2" pattern="$3"
  must_exist "${path}" || return
  if grep -nE "${pattern}" "${path}" >/dev/null 2>&1; then
    echo "FAIL: ${desc} — found '${pattern}' in ${path}:"
    grep -nE "${pattern}" "${path}" || true
    fail=1
  else
    echo "OK: ${desc}"
  fi
}

# Require pattern in file
require() {
  local desc="$1" path="$2" pattern="$3"
  must_exist "${path}" || return
  if grep -nE "${pattern}" "${path}" >/dev/null 2>&1; then
    echo "OK: ${desc}"
  else
    echo "FAIL: ${desc} — expected '${pattern}' in ${path}"
    fail=1
  fi
}

WF=".github/workflows/crates-publish.yml"
BF=".github/workflows/crates-publish-backfill.yml"
SH="scripts/publish_crates_backfill.sh"

require "publish workflow present" "${WF}" "name: Publish crates.io"
forbid "no --no-verify in publish workflow" "${WF}" "--no-verify"
forbid "no skip_tests input" "${WF}" "skip_tests"
forbid "no default master ref" "${WF}" 'default: "master"'
forbid "no git_ref branch input" "${WF}" "git_ref"
require "tag-only input present" "${WF}" "inputs:"
require "immutable tag regex present" "${WF}" 'v\[0-9\]'
require "cargo test required" "${WF}" "cargo test --workspace --locked"
require "cargo publish present" "${WF}" "cargo publish"
require "tag/commit bind step" "${WF}" "tag_commit"

forbid "backfill script no --no-verify" "${SH}" "--no-verify"
forbid "backfill version_check not ignored" "${SH}" 'version_check\.sh \|\| true'
require "backfill version_check invoked" "${SH}" "version_check.sh"

if [ -f "${BF}" ]; then
  forbid "backfill workflow no --no-verify" "${BF}" "--no-verify"
fi

if [ "${fail}" -ne 0 ]; then
  echo "crates publish policy check FAILED" >&2
  exit 1
fi
echo "crates publish policy check OK"
