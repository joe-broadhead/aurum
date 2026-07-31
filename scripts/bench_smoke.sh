#!/usr/bin/env bash
# PR-safe micro-benchmarks (JOE-1606). No model downloads.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export AURUM_BENCH_COMMIT="${AURUM_BENCH_COMMIT:-$(git rev-parse --short HEAD 2>/dev/null || echo unknown)}"
cargo test -p aurum-core --lib bench:: -- --nocapture
echo "bench smoke ok (commit=$AURUM_BENCH_COMMIT)"
