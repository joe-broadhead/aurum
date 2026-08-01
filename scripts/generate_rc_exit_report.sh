#!/usr/bin/env bash
# Assemble 1.0 RC exit report (JOE-1904).
# Machine-checkable rows only; human sign-off left blank.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${1:-dist/rc-exit}"
mkdir -p "${OUT}"
STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
VER="$(tr -d '[:space:]' < VERSION)"
COMMIT="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
REPORT="${OUT}/RC_EXIT_REPORT.md"

check_file() {
  local p="$1"
  if [ -f "$p" ]; then echo "present"; else echo "MISSING"; fi
}

run_ok() {
  if "$@" >/dev/null 2>&1; then echo "pass"; else echo "fail"; fi
}

echo "== generate RC exit report =="

# Optional green checks (do not fail report generation if a check fails;
# record status in the table). Capture individually.
FREEZE_ST="$(run_ok ./scripts/rc_freeze_check.sh)"
SBOM_ST="$(run_ok ./scripts/generate_sbom.sh "${OUT}/sbom")"
DOWNSTREAM_ST="$(run_ok ./scripts/rc_downstream_check.sh)"
REHEARSE_ST="$(run_ok ./scripts/rehearse_rc_rollback.sh)"

NATIVE_ST="missing"
if [ -f "${OUT}/sbom/native-components.md" ]; then
  NATIVE_ST="present"
elif [ -f dist/sbom/native-components.md ]; then
  NATIVE_ST="present"
fi

{
  echo "# Aurum 1.0 RC exit report"
  echo
  echo "- **generated_at_utc:** ${STAMP}"
  echo "- **workspace_version:** ${VER}"
  echo "- **source_commit:** ${COMMIT}"
  echo "- **generator:** scripts/generate_rc_exit_report.sh (JOE-1904)"
  echo
  echo "## Machine-checkable evidence"
  echo
  echo "| Area | Artifact / check | Status |"
  echo "|------|-----------------|--------|"
  echo "| Freeze inventory | docs/operations/rc-freeze.md | $(check_file docs/operations/rc-freeze.md) |"
  echo "| Freeze automated | scripts/rc_freeze_check.sh | ${FREEZE_ST} |"
  echo "| Dogfood checklist | docs/operations/rc-dogfood.md | $(check_file docs/operations/rc-dogfood.md) |"
  echo "| Rollback rehearsal | docs/operations/rc-rollback.md | $(check_file docs/operations/rc-rollback.md) |"
  echo "| Support policy | docs/operations/support-policy.md | $(check_file docs/operations/support-policy.md) |"
  echo "| Threat model matrix | docs/operations/threat-model.md | $(check_file docs/operations/threat-model.md) |"
  echo "| External review brief | docs/operations/external-review-brief.md | $(check_file docs/operations/external-review-brief.md) |"
  echo "| Disclosure tabletop | docs/operations/disclosure-tabletop.md | $(check_file docs/operations/disclosure-tabletop.md) |"
  echo "| Model revocation | docs/operations/model-revocation.md | $(check_file docs/operations/model-revocation.md) |"
  echo "| Provenance / cosign | docs/operations/provenance.md | $(check_file docs/operations/provenance.md) |"
  echo "| QE depth | docs/operations/qe-depth.md | $(check_file docs/operations/qe-depth.md) |"
  echo "| SBOM generate | scripts/generate_sbom.sh | ${SBOM_ST} |"
  echo "| Native inventory | native-components.md | ${NATIVE_ST} |"
  echo "| Downstream consumers | scripts/rc_downstream_check.sh | ${DOWNSTREAM_ST} |"
  echo "| Rollback dry-run | scripts/rehearse_rc_rollback.sh | ${REHEARSE_ST} |"
  echo
  echo "## Quality / security CI map (reference)"
  echo
  echo "See docs/operations/release-gate.md for the full gate table (fuzz, Miri,"
  echo "sanitizer, coverage, mutants, clean-install, repro, independent verify)."
  echo
  echo "## Human sign-off (required for 1.0 — leave blank in automation)"
  echo
  echo "| Field | Value |"
  echo "|-------|--------|"
  echo "| Candidate release tag | |"
  echo "| Freeze inventory reviewed | |"
  echo "| Tier A dogfood evidence complete | |"
  echo "| External review High/Critical closed | |"
  echo "| Residual risks accepted | |"
  echo "| **Approver name** | |"
  echo "| **Approve 1.0 release?** | |"
  echo "| Date (UTC) | |"
  echo
  echo "## Definition of Done (JOE-1655)"
  echo
  echo "A 1.0 cut requires this report plus human approval of one immutable"
  echo "source/tag/artifact/schema/document set. Automation cannot self-declare."
  echo
} > "${REPORT}"

echo "Wrote ${REPORT}"
# Fail the script if critical automated checks failed so CI is red.
if [ "${FREEZE_ST}" != "pass" ] || [ "${SBOM_ST}" != "pass" ] || [ "${DOWNSTREAM_ST}" != "pass" ]; then
  echo "RC exit report recorded failures (freeze=${FREEZE_ST} sbom=${SBOM_ST} downstream=${DOWNSTREAM_ST})" >&2
  cat "${REPORT}"
  exit 1
fi
cat "${REPORT}"
echo "generate_rc_exit_report.sh OK"
