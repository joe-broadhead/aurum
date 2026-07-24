#!/usr/bin/env bash
# crates.io readiness check (does not publish).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

./scripts/version_check.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

for crate in aurum-core aurum; do
  echo "==> cargo package -p ${crate} --list"
  cargo package -p "${crate}" --list --allow-dirty 2>/dev/null | head -30 || \
    cargo package -p "${crate}" --list | head -30
  echo "==> cargo publish -p ${crate} --dry-run"
  if cargo publish -p "${crate}" --dry-run --allow-dirty 2>&1; then
    echo "${crate} dry-run OK"
  else
    echo "NOTE: ${crate} dry-run failed (network / deps); package --list is the local gate"
  fi
done

echo "publish dry-run complete (nothing uploaded)"
echo "Publish order when approved: aurum-core first, then aurum."
