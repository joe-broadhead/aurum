# RC rollback, yank, revocation, notification rehearsal (JOE-1895)

**Document version:** 1.0  
**Mode:** non-production tabletop + dry-run scripts (never rewrites published tags)

## Principles

1. **Supersede, do not rewrite** release tags or Git history of published assets.
2. **Yank crates.io** only for safety-critical broken builds; prefer advisory + new tag.
3. **Cosign/SBOM** of the superseding tag are the new trust root.
4. **Model pin revoke** follows [model-revocation.md](model-revocation.md).
5. **Human sign-off** is required for 1.0 RC exit — automation cannot self-declare.

## Scenarios rehearsed

| ID | Scenario | Dry-run action |
|----|----------|----------------|
| RB-01 | Bad binary discovered after publish | Document superseding tag plan; do not delete old tag |
| RB-02 | crates.io yank decision tree | Checklist only (no yank without human) |
| RB-03 | Model pin compromise | `rehearse_model_revocation.sh` |
| RB-04 | Vulnerability disclosure publish | `rehearse_disclosure_tabletop.sh` |
| RB-05 | User notification text | Template in evidence pack |

## Operator procedure (production)

1. **Stop** recommending the bad tag in docs/README if needed.
2. **Triage** severity; open private advisory if security-related.
3. **Fix** on a branch; green CI + `release_gate.sh`.
4. **Publish** `vX.Y.Z+1` (or next 0.0.x) with CHANGELOG Security/Fixed section.
5. **Verify** independent `release-verify.yml` / `independent_release_verify.sh`.
6. **Notify** — GitHub Release notes, optional crates.io yank, advisory.
7. **Quarantine** local caches if model-related (`aurum cache verify`).
8. **Record** evidence under `dist/security-rehearsal/` or RC exit binder.

## Dry-run

```bash
./scripts/rehearse_rc_rollback.sh
```

Writes `dist/security-rehearsal/RC_ROLLBACK_REHEARSAL.md` with sign-off fields.

## Human sign-off (1.0 RC exit)

| Field | Value |
|-------|--------|
| RC tag under evaluation | |
| Freeze inventory version | docs/operations/rc-freeze.md |
| Dogfood evidence complete | yes / no |
| Rollback rehearsal date | |
| Independent verify run URL | |
| Residual risks accepted | |
| **Approver name** | |
| **Approve 1.0 cut?** | yes / no |
| Date (UTC) | |

Automation **must not** fill the approver fields.

## Related

* [model-revocation.md](model-revocation.md) · [disclosure-tabletop.md](disclosure-tabletop.md)
* [provenance.md](provenance.md) · [release-gate.md](release-gate.md) · [support-policy.md](support-policy.md)
