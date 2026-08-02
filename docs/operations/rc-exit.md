# RC exit report (JOE-1904 / JOE-2226) — v0.0.22 programme

**Purpose:** single binder linking JOE-2215 product-outcomes evidence for the
**v0.0.22** cut. Automation fills machine-checkable rows; **humans** fill
sign-off. See also [v0022-product-acceptance.md](v0022-product-acceptance.md).

## Generate

```bash
./scripts/generate_rc_exit_report.sh [OUT_DIR]
# default: dist/rc-exit/RC_EXIT_REPORT.md
```

CI job `rc-exit-pack` runs freeze + downstream + native SBOM + product contracts
+ provider evidence + exit report and uploads the artifact.

## Human sign-off (required for v0.0.22)

| Field | Value |
|-------|--------|
| Candidate tag | |
| Freeze inventory OK | |
| Product acceptance index reviewed | |
| Dogfood Tier A complete | |
| Provider tiers accepted | |
| Residual risks accepted | |
| **Approver name** | |
| **Approve v0.0.22 release?** | yes / no |
| Date (UTC) | |

Automation must leave approver fields blank.

## Related

* [rc-freeze.md](rc-freeze.md) · [rc-dogfood.md](rc-dogfood.md) · [rc-rollback.md](rc-rollback.md)
* [v0022-product-acceptance.md](v0022-product-acceptance.md) · [release-gate.md](release-gate.md)
* [provider-qualification.md](provider-qualification.md)
