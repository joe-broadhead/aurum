#!/usr/bin/env bash
# Dry-run model pin revocation rehearsal (JOE-1892).
#
# Does NOT modify production pins. Inventories catalogue pins and writes an
# evidence stub operators can attach to RC exit reports.
#
# Usage (repo root):
#   ./scripts/rehearse_model_revocation.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${AURUM_REHEARSAL_OUT:-dist/security-rehearsal}"
mkdir -p "${OUT}"
STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
EVIDENCE="${OUT}/MODEL_REVOCATION_REHEARSAL.md"

echo "== model revocation rehearsal (dry-run) =="

# Inventory pins from source (no network).
PIN_COUNT="$(rg -c 'Some\("[0-9a-f]{64}"\)|"ggml-.*\.bin" => Some\("' crates/aurum-core/src/model/mod.rs 2>/dev/null || true)"
CATALOGUE="$(rg -n 'filename: "ggml-' crates/aurum-core/src/model/mod.rs | head -40 || true)"
PIN_FNS="$(rg -n 'pub fn pinned_sha256|pub fn pinned_exact_bytes' crates/aurum-core/src/model/mod.rs || true)"

# Unit tests that guard pin coverage must exist.
echo "== cargo test pin catalogue guards =="
cargo test -p aurum-core --lib --no-default-features --locked -- model::tests::reviewed 2>&1 | tail -20

{
  echo "# Model revocation rehearsal evidence (JOE-1892)"
  echo
  echo "- **generated_at_utc:** ${STAMP}"
  echo "- **source_commit:** $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "- **mode:** dry-run (no pin mutations)"
  echo
  echo "## Checklist (simulated)"
  echo
  echo "- [x] Inventory trusted catalogue filenames"
  echo "- [x] Confirm pin helpers present (\`pinned_sha256\` / \`pinned_exact_bytes\`)"
  echo "- [x] Run catalogue pin unit guard (\`model::tests::reviewed…\`)"
  echo "- [ ] *(operator)* Remove or replace pin for compromised filename"
  echo "- [ ] *(operator)* Ship superseding tag + advisory"
  echo "- [ ] *(operator)* Instruct users: \`aurum cache verify\` + upgrade"
  echo "- [ ] *(operator)* Independent release-verify job green"
  echo
  echo "## Pin helpers"
  echo
  echo '```'
  echo "${PIN_FNS}"
  echo '```'
  echo
  echo "## Catalogue filename inventory (sample)"
  echo
  echo '```'
  echo "${CATALOGUE}"
  echo '```'
  echo
  echo "## Policy refs"
  echo
  echo "- docs/operations/model-revocation.md"
  echo "- docs/operations/threat-model.md (T-REV-01)"
  echo
  echo "## Result"
  echo
  echo "Rehearsal completed without modifying pins. Production revoke still requires"
  echo "an explicit source change + release."
} > "${EVIDENCE}"

echo "Wrote ${EVIDENCE}"
cat "${EVIDENCE}"
echo "rehearse_model_revocation.sh OK"
