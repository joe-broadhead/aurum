#!/usr/bin/env bash
# RC rollback / yank / revocation / notification rehearsal (JOE-1895).
#
# Dry-run only: composes evidence from model-revocation + disclosure rehearsals
# and a rollback decision table. Never yanks crates.io or deletes tags.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${AURUM_REHEARSAL_OUT:-dist/security-rehearsal}"
mkdir -p "${OUT}"
STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
EVIDENCE="${OUT}/RC_ROLLBACK_REHEARSAL.md"

echo "== RC rollback rehearsal (dry-run) =="

chmod +x scripts/rehearse_model_revocation.sh scripts/rehearse_disclosure_tabletop.sh
./scripts/rehearse_model_revocation.sh
./scripts/rehearse_disclosure_tabletop.sh

# Confirm we refuse to rewrite tags in docs/scripts (spot-check).
if rg -n 'git push --force|git tag -d|delete.*release' scripts/release*.sh docs/operations/rc-rollback.md 2>/dev/null | rg -v 'do not|never|not rewrite|Supersede'; then
  echo "unexpected destructive release language found" >&2
  exit 1
fi

{
  echo "# RC rollback rehearsal evidence (JOE-1895)"
  echo
  echo "- **generated_at_utc:** ${STAMP}"
  echo "- **source_commit:** $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "- **mode:** dry-run (no yank, no tag delete)"
  echo
  echo "## Scenarios"
  echo
  echo "| ID | Scenario | Result |"
  echo "|----|----------|--------|"
  echo "| RB-01 | Supersede bad tag | Documented: publish next tag only |"
  echo "| RB-02 | crates.io yank tree | Human decision only; not automated |"
  echo "| RB-03 | Model pin revoke | rehearse_model_revocation.sh OK |"
  echo "| RB-04 | Disclosure publish | rehearse_disclosure_tabletop.sh OK |"
  echo "| RB-05 | Notification template | See below |"
  echo
  echo "## Sample user notification (template)"
  echo
  echo '```'
  echo "Aurum vX.Y.Z is superseded by vX.Y.Z+1 due to <reason>."
  echo "Please upgrade, re-run: aurum cache verify"
  echo "Verify downloads with AURUM_REQUIRE_COSIGN=1 ./scripts/verify_release_assets.sh"
  echo "Details: <advisory-or-release-notes-url>"
  echo '```'
  echo
  echo "## Human sign-off (1.0 RC exit) — leave blank in automation"
  echo
  echo "| Field | Value |"
  echo "|-------|--------|"
  echo "| RC tag under evaluation | |"
  echo "| Freeze inventory reviewed | |"
  echo "| Dogfood evidence complete | |"
  echo "| This rehearsal reviewed | |"
  echo "| Residual risks accepted | |"
  echo "| **Approver name** | |"
  echo "| **Approve 1.0 cut?** | |"
  echo "| Date (UTC) | |"
  echo
  echo "## Related evidence"
  echo
  echo "- ${OUT}/MODEL_REVOCATION_REHEARSAL.md"
  echo "- ${OUT}/DISCLOSURE_TABLETOP.md"
  echo "- docs/operations/rc-rollback.md"
  echo
  echo "Rehearsal complete. Automation must not fill approver fields."
} > "${EVIDENCE}"

echo "Wrote ${EVIDENCE}"
cat "${EVIDENCE}"
echo "rehearse_rc_rollback.sh OK"
