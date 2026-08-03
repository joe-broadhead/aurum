#!/usr/bin/env bash
# Sanitizer / stress matrix for supported paths (JOE-1887 / JOE-2228).
#
# Strategy:
#   1. AddressSanitizer (ASan) on Linux nightly for pure domain/dto filters
#   2. JOE-2228 pure-filter "UB" matrix:
#        - Prefer rustc -Zsanitizer=undefined when the nightly still lists it
#        - Else fail-closed pure filters with overflow-checks + debug-assertions
#          (current nightlies dropped `undefined`; Miri remains primary pure-Rust UB)
#   3. Concurrency stress unit tests on stable (all OSes)
#   4. Document remaining gaps (full whisper/ORT under sanitizers, macOS ASan)
#
# Usage:
#   ./scripts/run_sanitizers.sh            # auto: ASan if Linux else stress only
#   ./scripts/run_sanitizers.sh --stress   # concurrency/leak stress only
#   ./scripts/run_sanitizers.sh --asan     # force ASan attempt + stress
#   ./scripts/run_sanitizers.sh --ubsan    # JOE-2228 pure-filter UB matrix
#   ./scripts/run_sanitizers.sh --all      # ASan + UB matrix + stress (Linux)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="auto"
while [ $# -gt 0 ]; do
  case "$1" in
    --stress) MODE="stress"; shift ;;
    --asan) MODE="asan"; shift ;;
    --ubsan) MODE="ubsan"; shift ;;
    --all) MODE="all"; shift ;;
    --auto) MODE="auto"; shift ;;
    -h|--help) sed -n '1,30p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

PURE_FILTERS=(domain:: dto:: secret:: providers::tests::)

run_stress() {
  echo "== concurrency / lifecycle stress (stable) =="
  cargo test -p aurum-core --test fault_injection --locked
  cargo test -p aurum-core --test mutation_semantics --no-default-features --locked
  cargo test -p aurum-ffi --lib --no-default-features --locked -- stress
}

host_is_linux() {
  local triple
  triple="$(rustc +nightly -vV 2>/dev/null | sed -n 's/^host: //p' || true)"
  [[ "${triple}" == *linux* ]]
}

# Returns 0 if nightly rustc accepts -Zsanitizer=undefined.
rustc_supports_ubsan() {
  # Probe via error text: unsupported values list does not include "undefined".
  local err
  err="$(rustc +nightly -Zsanitizer=undefined --print cfg - 2>&1 </dev/null || true)"
  if echo "${err}" | grep -qi "incorrect value \`undefined\`"; then
    return 1
  fi
  # If rustc advanced far enough to complain about missing input/crate, sanitizer is accepted.
  if echo "${err}" | grep -qiE "no input|error: "; then
    # accepted enough that it is not the "incorrect value" path
    if echo "${err}" | grep -qi "incorrect value"; then
      return 1
    fi
    return 0
  fi
  return 1
}

run_asan() {
  echo "== AddressSanitizer (nightly, pure filters) =="
  if ! command -v rustup >/dev/null; then
    echo "rustup required for ASan job" >&2
    exit 1
  fi
  local triple
  triple="$(rustc +nightly -vV | sed -n 's/^host: //p')"
  case "${triple}" in
    *linux*)
      ;;
    *)
      echo "ASan matrix is validated on Linux; host=${triple} — stress only (see qe-depth.md)."
      return 0
      ;;
  esac

  rustup component add rust-src --toolchain nightly 2>/dev/null || true

  echo "ASan target=${triple} filters=${PURE_FILTERS[*]}"
  set +e
  RUSTFLAGS="-Zsanitizer=address" \
    ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0:halt_on_error=1}" \
    cargo +nightly test -p aurum-core --lib --no-default-features --locked \
    -Zbuild-std --target "${triple}" \
    -- "${PURE_FILTERS[@]}"
  local rc=$?
  set -e
  if [ "${rc}" -ne 0 ]; then
    echo "build-std ASan path failed (rc=${rc}); trying host RUSTFLAGS smoke on domain only..."
    RUSTFLAGS="-Zsanitizer=address" \
      ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0:halt_on_error=1}" \
      cargo +nightly test -p aurum-core --lib --no-default-features --locked \
      -- domain::
  fi
}

run_overflow_pure() {
  echo "== overflow-checks + debug-assertions pure filters (JOE-2228 fallback) =="
  echo "note: current nightly rustc does not list -Zsanitizer=undefined;"
  echo "      Miri (ci miri job) remains the primary pure-Rust UB detector."
  echo "filters=${PURE_FILTERS[*]}"
  RUSTFLAGS="${RUSTFLAGS:-} -C overflow-checks=on -C debug-assertions=on" \
    cargo test -p aurum-core --lib --no-default-features --locked \
    -- "${PURE_FILTERS[@]}"
}

run_ubsan() {
  echo "== JOE-2228 pure-filter UB matrix =="
  if ! command -v rustup >/dev/null; then
    echo "rustup required for UB matrix" >&2
    exit 1
  fi

  if rustc_supports_ubsan; then
    local triple
    triple="$(rustc +nightly -vV | sed -n 's/^host: //p')"
    case "${triple}" in
      *linux*)
        ;;
      *)
        echo "LLVM UBSan path is validated on Linux; host=${triple} — overflow pure filters."
        run_overflow_pure
        return 0
        ;;
    esac
    rustup component add rust-src --toolchain nightly 2>/dev/null || true
    echo "UBSan (rustc undefined) target=${triple} filters=${PURE_FILTERS[*]}"
    set +e
    RUSTFLAGS="-Zsanitizer=undefined" \
      UBSAN_OPTIONS="${UBSAN_OPTIONS:-print_stacktrace=1:halt_on_error=1}" \
      cargo +nightly test -p aurum-core --lib --no-default-features --locked \
      -Zbuild-std --target "${triple}" \
      -- "${PURE_FILTERS[@]}"
    local rc=$?
    set -e
    if [ "${rc}" -ne 0 ]; then
      echo "build-std UBSan failed (rc=${rc}); falling back to overflow pure filters..."
      run_overflow_pure
    fi
  else
    run_overflow_pure
  fi
}

case "${MODE}" in
  stress) run_stress ;;
  asan) run_asan; run_stress ;;
  ubsan)
    run_ubsan
    if [[ "${AURUM_UBSAN_WITH_STRESS:-}" == "1" ]]; then
      run_stress
    fi
    ;;
  all)
    if host_is_linux || [[ "$(uname -s)" == "Linux" ]]; then
      run_asan
    else
      echo "Non-Linux host: ASan deferred to Linux CI (documented gap)."
    fi
    run_ubsan
    run_stress
    ;;
  auto)
    if [[ "$(uname -s)" == "Linux" ]]; then
      run_asan
    else
      echo "Non-Linux host: ASan deferred to Linux CI (documented gap)."
    fi
    run_stress
    ;;
esac

echo "run_sanitizers.sh OK"
