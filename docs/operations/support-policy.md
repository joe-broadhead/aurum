# Support, security fix, deprecation, and first-patch policy (JOE-1898)

**Document version:** 1.1  
**Applies to:** Aurum on the continuous **0.0.x** line, including the **v0.0.21**
production-assurance cut (formerly labelled “1.0 RC”).

## Maintainer ownership

| Area | Owner |
|------|--------|
| Releases / tags / cosign | Primary maintainer (repository owner) |
| Security advisories | Primary maintainer via GitHub Security Advisories |
| Docs / handbook | Primary maintainer |
| FFI ABI | Primary maintainer; bumps require ABI version review |

## Support policy (v0.0.x / assurance cuts)

* **Supported install:** GitHub Release Tier A binaries; `cargo install aurum-stt --locked` Tier B.
* **Best-effort:** experimental platforms (Tier C), unpinned BYOM, multi-tenant without sandbox; experimental remote providers (see [provider-matrix.md](../guide/provider-matrix.md)).
* **Not supported as product guarantee:** multi-tenant isolation inside one process ([threat-model.md](threat-model.md) T-MT-01); remote execution on the C ABI.
* Issues: GitHub Issues for non-security bugs; security only via private path ([SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md)).

## Security-fix cadence

| Severity | Target response |
|----------|-----------------|
| Critical (RCE, secret leak on default path) | Immediate private triage; patch release ASAP |
| High | Patch in next 0.0.x; advisory when coordinated |
| Medium | Scheduled fix; documented residual if deferred |
| Low | Backlog |

Default local path (no cloud keys) is prioritized over optional remote surfaces.

## Deprecation cadence

* Prefer **one release** of deprecation notice before removing public Rust APIs.
* C ABI: bump `AURUM_ABI_VERSION` and set `AURUM_ABI_MIN_VERSION` to the same value on greenfield cuts.
* During **RC freeze**, breaking changes reset the freeze ([rc-freeze.md](rc-freeze.md)).

## Incident response

1. Private intake → triage → fix → verify → signed release → notify (see [disclosure-tabletop.md](disclosure-tabletop.md)).
2. Supply chain: cosign identity rotation notes in [provenance.md](provenance.md).
3. Model pins: [model-revocation.md](model-revocation.md).
4. Credentials: [credential-rotation-runbook.md](credential-rotation-runbook.md).

## Backup / cache recovery

| Asset | Recovery |
|-------|----------|
| Model cache | `aurum cache verify` / re-download; quarantine retains forensics |
| User transcripts | User-owned paths; Aurum does not cloud-backup |
| Release artifacts | Re-download from GitHub Release; verify cosign |

## First-patch readiness (post v0.0.21)

Before calling the **v0.0.21** assurance cut final:

- [ ] RC freeze inventory current
- [ ] Dogfood evidence for Tier A
- [ ] Rollback rehearsal complete with human sign-off block blank→filled
- [ ] SECURITY.md + this policy linked from handbook
- [ ] Ability to cut a patch release (`0.0.22+`) within one working day of Critical fix

## Related

* [handbook.md](handbook.md) · [release-gate.md](release-gate.md) · [threat-model.md](threat-model.md)
* [compatibility.md](../development/compatibility.md)
