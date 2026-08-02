# External review disposition — post-provider remediation (2026-08-02)

**Programme:** [JOE-1974](https://linear.app/joe-broadhead/issue/JOE-1974) / [JOE-1913](https://linear.app/joe-broadhead/issue/JOE-1913)  
**Pre-provider freeze (historical):** tag `v0.0.18` — do **not** treat as covering the provider platform  
**Post-provider candidate (mutable tip until freeze):** `master` after JOE-1975–1980 remediation merges  
**Owner:** Joseph Broadhead  

This table reconciles the automated post-v0.0.18 review (NO for 1.0 on `dee9fcc…`) with
code landings. It does **not** replace [JOE-1920](https://linear.app/joe-broadhead/issue/JOE-1920)
qualified human sign-off on an immutable post-provider tag.

## Finding summary

| ID | Linear | Severity | Disposition | Notes |
|----|--------|----------|-------------|-------|
| F-001 | JOE-1914 | High | **Fixed** (prior) + retest required on new freeze | Canary matrix must include all remote providers |
| F-002 | JOE-1915 + JOE-1715 | High | **Fixed** (prior) + retest required on new freeze | Publish-policy + operator rotation |
| F-003 | JOE-1916 | Medium | **Fixed** + accepted residual | Free-form notes |
| F-004 | JOE-1917 | Medium | **Fixed** + accepted residual | Multi-tenant unsupported |
| F-005 | JOE-1918 | Medium | **Fixed** (PR #56 + **verified-snap staging**) | Native load opens process-owned snap for digested artifacts |
| F-006 | JOE-1919 | Medium | **Fixed** (PR #57) + **accepted residual** | Full SLSA / crates.io package attestation not claimed |
| NP-001 | JOE-1975 | High | **Fixed** (PR #77) | Engine remote permit + interruptible HTTP |
| NP-002 | JOE-1976 | Medium | **Fixed** (PR #78) | Official xAI `/v1/stt` + `/v1/tts` |
| NP-003 | JOE-1977 | Medium | **Fixed** (PR #78) | Fail-closed TTS MIME |
| NP-004 | JOE-1978 | Medium | **Fixed** (PR #79) | Exact OpenRouter TTS default; experimental until smoke |
| NP-005 | JOE-1979 | Medium | **Fixed** (PR #79) | ProviderId Serde, path encode, route-aware caps |
| NP-006 | JOE-1980 | Medium | **Fixed** (PR #79) | SecretString + public_network_reason |

## F-005 residual (updated)

**Code:** `stage_verified_for_load` copies digested artifacts into
`{cache}/tts/verified-snaps/<sha256>/` and re-hashes before ORT/NPZ open.
Source pack mutation after staging does not affect the snap.

**Accepted residual:**

- `local_unverified` packs without digests still load from the pack path after
  symlink checks only (hostile shared FS remains out of Tier A).
- An attacker who can write the process cache root can still attack snaps;
  cache root is single-user desktop assumed.

## F-006 residual (explicit, unchanged claim)

| Claim | Status |
|-------|--------|
| GitHub Release SHA256SUMS + cosign keyless | Required |
| PROVENANCE full 40-hex source_commit | Required (`verify_release_assets.sh`) |
| Full SLSA build attestation per artifact | **Not claimed** |
| crates.io package digests in same evidence set | **Not claimed** |
| Bit-for-bit dual-builder reproducibility | **Not claimed** where variance documented |

## Freeze gate

See [post-provider-security-freeze.md](post-provider-security-freeze.md) (JOE-1981).
Do not cut a tag until human approval; do not mark JOE-1655 Done from this file alone.
