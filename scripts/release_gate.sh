#!/usr/bin/env bash
# v0.0.3 / v1.0 release gate checks (JOE-1640 / JOE-1578).
# Fail-closed: any step failure aborts. Does not tag or publish.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== version sync =="
./scripts/version_check.sh

echo "== action pins =="
./scripts/check_action_pins.sh

echo "== crates.io publish policy (JOE-1915) =="
./scripts/check_crates_publish_policy.sh

echo "== fmt =="
cargo fmt --all -- --check

echo "== clippy =="
cargo clippy --workspace --all-targets --locked -- -D warnings

echo "== tests (default features) =="
cargo test --workspace --locked

echo "== adversarial suites =="
cargo test -p aurum-core --test adversarial_parsers --locked
cargo test -p aurum-core --test fault_injection --locked

echo "== STT-only build (no default features on aurum-core) =="
cargo check -p aurum-core --no-default-features --locked
cargo check -p aurum-stt --no-default-features --locked

echo "== docs =="
if command -v python3 >/dev/null 2>&1; then
  python3 -m pip install -q -r docs/requirements.txt
  mkdocs build --strict
else
  echo "python3 missing; skip mkdocs (CI must run Docs job)"
fi

echo "== SBOM inventory =="
./scripts/generate_sbom.sh dist/sbom

echo "== cargo deny (if installed) =="
if command -v cargo-deny >/dev/null 2>&1; then
  cargo deny check
else
  echo "cargo-deny not installed locally; CI security job enforces it"
fi

echo "== cargo audit (if installed) =="
if command -v cargo-audit >/dev/null 2>&1; then
  cargo audit
else
  echo "cargo-audit not installed locally; CI security job enforces it"
fi

echo "Release gate OK (no tag, no publish)."
