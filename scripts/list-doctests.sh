#!/usr/bin/env bash
# Doctest identifiers as libtest prints them, matching scripts/list-universe.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --workspace --doc --locked -- --list 2>/dev/null | sed -n 's/^\(.*\): test$/\1/p'
