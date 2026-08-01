# 1.0 RC exit report (JOE-1904)

**Purpose:** single binder linking all JOE-1655 evidence. Automation fills
machine-checkable rows; **humans** fill sign-off.

## Generate

```bash
./scripts/generate_rc_exit_report.sh [OUT_DIR]
# default: dist/rc-exit/RC_EXIT_REPORT.md
```

CI job `rc-exit-pack` runs freeze + downstream + native SBOM + exit report and
uploads the artifact.

## Human sign-off (required for 1.0)

| Field | Value |
|-------|--------|
| Candidate tag | |
| Freeze inventory OK | |
| Dogfood Tier A complete | |
| External review disposition closed for High/Critical | |
| Residual risks accepted | |
| **Approver name** | |
| **Approve 1.0 release?** | yes / no |
| Date (UTC) | |

Automation must leave approver fields blank.

## Related

* [rc-freeze.md](rc-freeze.md) · [rc-dogfood.md](rc-dogfood.md) · [rc-rollback.md](rc-rollback.md)
* [external-review-brief.md](external-review-brief.md) · [release-gate.md](release-gate.md)
