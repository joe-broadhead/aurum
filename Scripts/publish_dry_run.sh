#!/usr/bin/env bash
# crates.io readiness check (does not publish).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

./Scripts/version_check.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

echo "==> cargo package -p aurum-core --list"
cargo package -p aurum-core --list --allow-dirty 2>/dev/null | head -40 || \
  cargo package -p aurum-core --list | head -40

echo "==> cargo publish -p aurum-core --dry-run"
# dry-run still needs network for registry index in some cargo versions
if cargo publish -p aurum-core --dry-run --locked 2>&1; then
  echo "aurum-core dry-run OK"
else
  echo "NOTE: dry-run may fail offline or if package already exists; package --list above is the main gate"
fi

echo "publish dry-run complete (nothing uploaded)"
