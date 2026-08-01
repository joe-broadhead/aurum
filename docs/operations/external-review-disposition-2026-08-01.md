# External review disposition — Medium findings (2026-08-01)

**Programme:** [JOE-1913](https://linear.app/joe-broadhead/issue/JOE-1913)  
**Freeze candidate:** release `v0.0.18` (this document ships with that freeze)  
**High findings:** F-001 / F-002 closed (code + independent executable retest + JOE-1715 operator).  
**Owner:** Joseph Broadhead  

This table is the formal residual package for Medium findings after code merge.
It does **not** replace the qualified human assessment required by JOE-1920.

## Summary table

| ID | Issue | Severity | Disposition | Owner | Residual risk | Target |
|----|-------|----------|-------------|-------|---------------|--------|
| F-003 | JOE-1916 | Medium | **Fixed** (PR #54) | Maintainer | Residual: untrusted `--notes` still free-form after scrub; operators must not paste secrets into notes. | Accepted for 0.0.x / 1.0 freeze |
| F-004 | JOE-1917 | Medium | **Fixed** (PR #55) | Maintainer | Residual: process-level governor does not replace OS cgroup limits in multi-tenant hosts (unsupported deployment). | Accepted; multi-tenant remains unsupported |
| F-005 | JOE-1918 | Medium | **Fixed** (PR #56) | Maintainer | Residual: hostile local filesystem races outside documented BYOM trust modes remain out of scope for Tier A local use. | Accepted under BYOM trust-mode docs |
| F-006 | JOE-1919 | Medium | **Fixed for 1.0 evidence gap stated in review** (PR #57) with **accepted residual** | Maintainer | Residual: full interoperable SLSA attestations per artifact and crates.io package provenance parity are **not** claimed; GitHub Release cosign + full-SHA PROVENANCE are required. | Track post-1.0 / optional hardening; not a High reopen |

## Detail

### F-003 — Doctor / support-bundle filesystem + privacy (JOE-1916)

**Finding:** predictable doctor probe paths and non-transactional support-bundle writes risk clobber/TOCTOU and incomplete path redaction.

**Fix:** exclusive `create_new` doctor probe; support bundle via `OutputTransaction`; expanded HOME/XDG path redaction; notes classified untrusted.

**Retest evidence:** unit tests under `doctor::` / `support::` in aurum-core; CI on merge.

**Residual acceptance:** free-form notes remain a human surface — scrubbed but not a secret store. Documented in CLI/help and this table.

### F-004 — ResourceGovernor construction / CPU overflow (JOE-1917)

**Finding:** invalid governor construction and unchecked CPU accounting.

**Fix:** validated construction; checked arithmetic for CPU accounting.

**Residual acceptance:** Aurum does not claim multi-tenant isolation; outer sandbox required for shared hosts (threat model / deployment profiles).

### F-005 — Artifact / BYOM TOCTOU (JOE-1918)

**Finding:** time-of-check/time-of-use and symlink gaps on artifact/BYOM paths.

**Fix:** reverify-before-load, exclusive partials, durable publish paths as implemented in PR #56.

**Residual acceptance:** BYOM remains explicit-trust; hostile admin on the same machine is out of Tier A local threat model.

### F-006 — SBOM / provenance identity (JOE-1919)

**Finding:** provenance/commit binding too loose for 1.0 evidence claims.

**Fix:** require full 40-char commit in generate/verify; richer PROVENANCE metadata.

**Residual acceptance (explicit):**

- Not claiming full SLSA level for every crate tarball on crates.io.
- GitHub Release path: checksums + cosign keyless + PROVENANCE full-SHA binding required.
- Optional future: OIDC trusted publishing for crates.io (does not reopen F-002 High).

## High findings (reference only)

| ID | Issue | Status |
|----|-------|--------|
| F-001 | JOE-1914 | Fixed + executable retest PASS (`c1b6b13` lineage; shipped in 0.0.18) |
| F-002 | JOE-1915 + JOE-1715 | Fixed + policy retest PASS + operator credential Done |

## Sign-off hooks

- Publishable human summary: update Linear JOE-1920 document after freeze tag exists.
- RC exit report: link this file + tag SHA when generating `generate_rc_exit_report.sh` for 1.0.
