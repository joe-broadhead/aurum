#!/usr/bin/env bash
# Static policy checks for crates.io publish workflows (JOE-1915 / JOE-1920 retest).
# Fails closed on policy violations **and** on checker/tool errors.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0
GREP_OUT="$(mktemp)"
GREP_ERR="$(mktemp)"
trap 'rm -f "${GREP_OUT}" "${GREP_ERR}"' EXIT

must_exist() {
  local path="$1"
  if [ ! -f "${path}" ]; then
    echo "FAIL: missing ${path}"
    fail=1
    return 1
  fi
  return 0
}

# Strip full-line comments for grepping executable surface (YAML/shell).
stripped() {
  local path="$1"
  sed -E \
    -e '/^[[:space:]]*#/d' \
    -e 's/[[:space:]]+#.*$//' \
    "${path}"
}

# Exit: 0 = match found, 1 = no match, 2+ = tool failure (must fail closed).
# Patterns with leading '-' are safe: always passed via -e (JOE-1920 fail-open fix).
# Does NOT toggle `set -e` (would leak into the caller).
grep_stripped() {
  local path="$1"
  local pattern="$2"
  local tmp st
  tmp="$(mktemp)"
  stripped "${path}" >"${tmp}"
  # Always pass pattern after -e so leading '-' is not a grep option.
  if grep -nE -e "${pattern}" "${tmp}" >"${GREP_OUT}" 2>"${GREP_ERR}"; then
    rm -f "${tmp}"
    return 0
  else
    st=$?
    rm -f "${tmp}"
    if [ "${st}" -eq 1 ]; then
      return 1
    fi
    return 2
  fi
}

forbid() {
  local desc="$1" path="$2" pattern="$3" st=0
  if ! must_exist "${path}"; then
    return 0
  fi
  # Capture status without set -e interaction.
  grep_stripped "${path}" "${pattern}" && st=0 || st=$?
  if [ "${st}" -eq 0 ]; then
    echo "FAIL: ${desc} — found '${pattern}' in executable surface of ${path}:"
    cat "${GREP_OUT}" || true
    fail=1
  elif [ "${st}" -eq 1 ]; then
    echo "OK: ${desc}"
  else
    echo "FAIL: ${desc} — grep error status ${st} (checker fail-closed)" >&2
    cat "${GREP_ERR}" >&2 || true
    fail=1
  fi
}

require() {
  local desc="$1" path="$2" pattern="$3" st=0
  if ! must_exist "${path}"; then
    return 0
  fi
  grep_stripped "${path}" "${pattern}" && st=0 || st=$?
  if [ "${st}" -eq 0 ]; then
    echo "OK: ${desc}"
  elif [ "${st}" -eq 1 ]; then
    echo "FAIL: ${desc} — expected '${pattern}' in ${path}"
    fail=1
  else
    echo "FAIL: ${desc} — grep error status ${st} (checker fail-closed)" >&2
    cat "${GREP_ERR}" >&2 || true
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
require "crate order guard present" "${WF}" "CRATE_ORDER|publish_order|aurum-core"

forbid "backfill script no --no-verify" "${SH}" "--no-verify"
forbid "backfill version_check not ignored" "${SH}" 'version_check\.sh \|\| true'
require "backfill version_check invoked" "${SH}" "version_check.sh"
require "backfill allowlist versions present" "${SH}" "ALLOWLIST"

if [ -f "${BF}" ]; then
  forbid "backfill workflow no --no-verify" "${BF}" "--no-verify"
fi

# Self-test: must detect --no-verify when present (prevents fail-open regression).
selftest_tmp="$(mktemp)"
printf '%s\n' 'cargo publish --no-verify' >"${selftest_tmp}"
st=0
grep_stripped "${selftest_tmp}" "--no-verify" && st=0 || st=$?
rm -f "${selftest_tmp}"
if [ "${st}" -eq 0 ]; then
  echo "OK: self-test detects --no-verify pattern"
else
  echo "FAIL: self-test — checker did not detect forbidden --no-verify (st=${st})" >&2
  fail=1
fi

if [ "${fail}" -ne 0 ]; then
  echo "crates publish policy check FAILED" >&2
  exit 1
fi
echo "crates publish policy check OK"
