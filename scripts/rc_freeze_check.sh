#!/usr/bin/env bash
# RC freeze inventory automated checks (JOE-1896).
#
# Verifies ABI constants, DTO schema versions, model pin catalogue, and
# mutation_semantics kill list still hold for the freeze inventory.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== rc_freeze_check (JOE-1896) =="

echo "== version sync =="
./scripts/version_check.sh

echo "== ABI layout (FFI) =="
cargo test -p aurum-ffi --test abi_layout --no-default-features --locked

echo "== DTO schema_version + domain freeze-related unit tests =="
cargo test -p aurum-core --lib --no-default-features --locked -- dto:: domain::

echo "== model pin catalogue guard =="
cargo test -p aurum-core --lib --no-default-features --locked -- model::tests::reviewed

echo "== mutation_semantics kill list =="
cargo test -p aurum-core --test mutation_semantics --no-default-features --locked

echo "== freeze inventory doc present =="
test -f docs/operations/rc-freeze.md

# Spot-check constants mentioned in inventory still match source.
python3 - <<'PY'
from pathlib import Path
types = Path("crates/aurum-ffi/src/types.rs").read_text()
dto = Path("crates/aurum-core/src/dto.rs").read_text()
assert "AURUM_ABI_VERSION: u32 = 2" in types or "pub const AURUM_ABI_VERSION: u32 = 2" in types
assert "STT_RESULT_SCHEMA_VERSION: u32 = 1" in dto
assert "TTS_META_SCHEMA_VERSION: u32 = 1" in dto
assert "ERROR_SCHEMA_VERSION: u32 = 1" in dto
print("schema/ABI constant spot-check OK")
PY

echo "== native inventory via SBOM (JOE-1902) =="
./scripts/generate_sbom.sh dist/sbom
test -f dist/sbom/native-components.md
grep -q 'whisper-rs' dist/sbom/native-components.md
grep -q 'ffmpeg' dist/sbom/native-components.md
echo "native-components.md OK"

echo "rc_freeze_check.sh OK"
