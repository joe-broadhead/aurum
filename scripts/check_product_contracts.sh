#!/usr/bin/env bash
# Fail if generated product contract surfaces drift (JOE-2224).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo run -q -p aurum-core --example generate_product_contracts -- --check
