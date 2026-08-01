# External security review brief (JOE-1901)

**Document version:** 1.0  
**Audience:** qualified independent reviewer  
**Product:** Aurum local-first STT/TTS (Rust workspace)

This brief is the **evidence pack**, not a substitute for an independent assessment.
Schedule human review against this checklist; record findings in the disposition
template (§3).

## 1. Scope for the reviewer

| Area | Why it matters | Primary artifacts |
|------|----------------|-------------------|
| Remote credentials | OpenRouter key handling, redaction | `secret`, credential hygiene, doctor JSON tests |
| Artifact / cache / BYOM | Model pins, quarantine, trust modes | model pins, cache verify, TTS BYOM docs |
| FFmpeg / native parsers | Hostile media | fuzz `wav_parse`, audio limits, doctor |
| Resource governance | Exhaustion / hang | ResourceGovernor, job stress tests |
| Filesystem transactions | Symlink/clobber races | output transaction + fault_injection |
| FFI / unsafe | Host embed safety | ABI tests, FFI examples, stress jobs |
| Observability / privacy | Log/support leakage | support redaction, threat-model T-KEY/T-LOG |
| Supply chain | Release authenticity | cosign, SBOM, independent verify workflow |

**Out of scope for product guarantees:** multi-tenant isolation without outer sandbox
([threat-model.md](threat-model.md) T-MT-01).

## 2. How to navigate evidence

| Topic | Doc / command |
|-------|----------------|
| Threat matrix | [threat-model.md](threat-model.md) |
| Disclosure path | [disclosure-tabletop.md](disclosure-tabletop.md), root SECURITY.md |
| Provenance / cosign | [provenance.md](provenance.md) |
| Model revoke | [model-revocation.md](model-revocation.md) |
| RC freeze | [rc-freeze.md](rc-freeze.md) |
| QE depth | [qe-depth.md](qe-depth.md) |
| Native inventory | release asset `native-components.md` / `generate_sbom.sh` |
| RC exit rollup | `scripts/generate_rc_exit_report.sh` → `RC_EXIT_REPORT.md` |

```bash
./scripts/rc_freeze_check.sh
./scripts/generate_sbom.sh dist/sbom
./scripts/generate_rc_exit_report.sh
# Optional against a published tag:
./scripts/independent_release_verify.sh --tag vX.Y.Z
```

## 3. Findings disposition template

Copy for each review engagement (no confidential details in public repos):

| ID | Severity | Area | Summary (redacted) | Status | Owner | Target date | Retest evidence |
|----|----------|------|--------------------|--------|-------|-------------|-----------------|
| F-001 | Critical / High / Medium / Low | | | Open / Fixed / Accepted residual | | | |

### Disposition rules (JOE-1655)

* **Critical / High:** must be fixed and independently retested before 1.0.
* **Medium:** reviewed disposition + target date allowed if residual risk accepted.
* **Low:** backlog OK with owner.

## 4. Suggested review timebox

1. Read threat-model control matrix + this brief (1–2 h).  
2. Spot-check pins, cosign verify, doctor offline, FFI smoke (2–4 h).  
3. Hostile media / fuzz corpus triage notes (variable).  
4. Write disposition table; schedule retest of High/Critical.

## Related

* [threat-model.md](threat-model.md) · [security.md](security.md) · [rc-exit.md](rc-exit.md)
