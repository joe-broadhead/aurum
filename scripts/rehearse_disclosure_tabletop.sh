#!/usr/bin/env bash
# Disclosure tabletop rehearsal (JOE-1890).
#
# Writes a filled sample evidence pack (fictional scenario) for RC exit binders.
# No secrets; no network required beyond optional cargo test.
#
# Usage:
#   ./scripts/rehearse_disclosure_tabletop.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${AURUM_REHEARSAL_OUT:-dist/security-rehearsal}"
mkdir -p "${OUT}"
STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
EVIDENCE="${OUT}/DISCLOSURE_TABLETOP.md"

echo "== disclosure tabletop rehearsal =="

# Lightweight green checks that would run during real disclosure verify step.
echo "== smoke tests used in verify step =="
cargo test -p aurum-core --test mutation_semantics --no-default-features --locked 2>&1 | tail -15
cargo test -p aurum-core --lib --no-default-features --locked -- secret:: 2>&1 | tail -10

{
  echo "# Disclosure tabletop evidence (JOE-1890)"
  echo
  echo "- **generated_at_utc:** ${STAMP}"
  echo "- **source_commit:** $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "- **scenario_id:** TT-2026-08-SAMPLE (fictional)"
  echo
  echo "## Intake checklist (completed in rehearsal)"
  echo
  echo "- [x] Private channel identified (GitHub Security Advisories)"
  echo "- [x] No public weaponized PoC before fix"
  echo "- [x] Affected surface classified (default local decode path)"
  echo "- [x] Severity sample: High (disk DoS via hostile media)"
  echo "- [x] Regression surface: fault_injection + fuzz wav_parse"
  echo "- [x] Verify commands exercised (mutation_semantics + secret tests)"
  echo "- [ ] *(live incident)* Coordinated advisory URL"
  echo "- [ ] *(live incident)* Superseding tag + independent release-verify"
  echo
  echo "## Sample timeline"
  echo
  echo "| Step | Result |"
  echo "|------|--------|"
  echo "| Receive | Private advisory |"
  echo "| Reproduce | Isolated; minimized input |"
  echo "| Fix | Fail-closed bounds (existing + regression) |"
  echo "| Verify | cargo tests green |"
  echo "| Publish | Tag + cosign + SBOM |"
  echo "| Notify | CHANGELOG Security + advisory |"
  echo
  echo "## Policy refs"
  echo
  echo "- SECURITY.md"
  echo "- docs/operations/disclosure-tabletop.md"
  echo "- docs/operations/threat-model.md (T-DISC-01, T-AUD-01)"
  echo
  echo "Rehearsal complete — fictional scenario only."
} > "${EVIDENCE}"

echo "Wrote ${EVIDENCE}"
cat "${EVIDENCE}"
echo "rehearse_disclosure_tabletop.sh OK"
