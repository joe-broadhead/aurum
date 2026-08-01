#!/usr/bin/env bash
# Run Miri on pure-Rust unit tests (JOE-1889 / JOE-1655).
#
# Full aurum-core is not Miri-clean: whisper.cpp, FFmpeg, Tokio async I/O, and ORT
# exercise unsupported foreign calls. This script runs a curated filter set that
# covers domain primitives, DTO validation, formatters (with isolation off),
# secret redaction, segment/DTO validation, and digest pin unit tests.
#
# Usage (from repo root):
#   ./scripts/run_miri.sh
#
# Env:
#   MIRIFLAGS   — extra Miri flags (default: isolation off for tempfile paths)
#   AURUM_MIRI_TOOLCHAIN — rustup toolchain (default: nightly)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TOOLCHAIN="${AURUM_MIRI_TOOLCHAIN:-nightly}"
# Isolation off: output transaction tests use tempfile/mkdir (documented).
export MIRIFLAGS="${MIRIFLAGS:--Zmiri-disable-isolation -Zmiri-permissive-provenance}"

echo "== Miri pure suite (toolchain=${TOOLCHAIN}) =="
echo "MIRIFLAGS=${MIRIFLAGS}"

if ! rustup run "${TOOLCHAIN}" miri --version >/dev/null 2>&1; then
  echo "Installing Miri component on ${TOOLCHAIN}..."
  rustup component add miri --toolchain "${TOOLCHAIN}"
fi
cargo "+${TOOLCHAIN}" miri setup

# Curated filters only (substring match). See docs/operations/qe-depth.md for gaps.
# Intentionally excluded: cleanup::rules (async/Tokio kqueue), audio::, remote::,
# full whisper/ORT paths.
FILTERS=(
  "domain::"
  "dto::"
  "output::tests::"
  # output::transaction uses hard_link/rename — unsupported under Miri (documented gap).
  "secret::"
  "providers::tests::"
  "model::tests::reviewed"
)

failed=0
ran=0
for f in "${FILTERS[@]}"; do
  echo
  echo "== miri filter: ${f} =="
  if cargo "+${TOOLCHAIN}" miri test -p aurum-core --lib --no-default-features --locked -- "${f}"; then
    ran=$((ran + 1))
  else
    echo "Miri FAILED for filter: ${f}" >&2
    failed=1
  fi
done

# Pure JobState mapping (no engine). May fail to link on some hosts — soft note.
echo
echo "== miri: JobState pure mapping (aurum-ffi, best-effort) =="
if cargo "+${TOOLCHAIN}" miri test -p aurum-ffi --lib --no-default-features --locked -- \
  "stress_job_state_u8_mapping" 2>&1; then
  ran=$((ran + 1))
else
  echo "NOTE: aurum-ffi Miri JobState filter skipped/failed (native link gap); core pure suite is authoritative."
fi

if [ "${failed}" -ne 0 ]; then
  echo "Miri suite reported failures" >&2
  exit 1
fi
if [ "${ran}" -lt 1 ]; then
  echo "Miri suite ran zero filters" >&2
  exit 1
fi

echo "run_miri.sh OK (${ran} filter groups)"
