# Confidential disclosure tabletop (JOE-1890)

**Document version:** 1.0  
**Purpose:** Rehearse the vulnerability response path without live secrets.  
**Audience:** maintainers + external reviewers preparing **v0.0.21** RC evidence.

## Tabletop script (sample scenario)

**Scenario ID:** `TT-2026-08-SAMPLE` (fictional — do not treat as a real vuln)

> A researcher privately reports that a crafted WAV causes an unbounded
> temporary file under `/tmp` during decode, potentially filling the disk.

### Roles

| Role | Name (sample) |
|------|----------------|
| Reporter | External (private advisory) |
| Receiver / owner | Maintainer |
| Verifier | Second maintainer or CI |

### Timeline (sample filled)

| Step | Action | Outcome (sample) |
|------|--------|------------------|
| 1. Intake | GitHub Security Advisory private report | Ack within 1 working day |
| 2. Reproduce | Isolated machine; minimized sample under embargo | Confirmed on v0.0.14 local path |
| 3. Severity | CVSS-style judgment; default local path impact | High (disk DoS) |
| 4. Fix | Private branch; fail-closed size cap already present — tighten precheck | Regression in `fault_injection` + fuzz seed |
| 5. Verify | `cargo test --workspace --locked`, audit, deny, `release_gate.sh` | Green |
| 6. Publish | Tag security release; cosign + SBOM | Independent verify job green |
| 7. Notify | Advisory + CHANGELOG Security section | Users told to upgrade |
| 8. Postmortem | Update threat model if needed | T-AUD-01 residual noted |

### Intake checklist (external reviewer usable)

- [ ] Report received via **private** channel (GitHub Security Advisories preferred)
- [ ] No public issue with weaponized PoC before fix/advisory
- [ ] Affected versions / install methods listed (CLI binary, crates.io, source)
- [ ] Default local path vs remote/FFI surfaces classified
- [ ] Severity + residual risk after fix recorded
- [ ] Regression test or fuzz seed added (or explicit waiver)
- [ ] Coordinated release notes + advisory published
- [ ] Independent release verify run linked
- [ ] Threat model / hardening updated if assumptions changed

## Commands used in verification (no secrets)

```bash
cargo test --workspace --locked
cargo audit && cargo deny check
./scripts/release_gate.sh
# After tag:
AURUM_REQUIRE_COSIGN=1 \
AURUM_EXPECT_TAG=vX.Y.Z \
AURUM_COSIGN_CERTIFICATE_OIDC_ISSUER=https://token.actions.githubusercontent.com \
AURUM_COSIGN_CERTIFICATE_IDENTITY="https://github.com/joe-broadhead/aurum/.github/workflows/release.yml@refs/tags/vX.Y.Z" \
./scripts/verify_release_assets.sh ./release-assets
```

## Evidence pack location

Rehearsal automation writes a stub under:

```text
dist/security-rehearsal/DISCLOSURE_TABLETOP.md
```

via `./scripts/rehearse_disclosure_tabletop.sh` (no network secrets required).

## Related

* Root [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md)
* [threat-model.md](threat-model.md) · [security.md](security.md) · [model-revocation.md](model-revocation.md)
