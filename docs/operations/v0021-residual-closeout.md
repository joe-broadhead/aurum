# v0.0.21 programme residual closeout (JOE-1655)

**Date (UTC):** 2026-08-02  
**Programme tip at closeout:** `master` @ `b9af1712bd251b119381360ccb58bd75f3ea6201` (workspace **0.0.20**)  
**Product cut target:** **v0.0.21** (continuous **0.0.x**; not 1.0)  
**Owner:** Joseph Broadhead  

This document closes the JOE-1655 production-assurance programme for shipping
**v0.0.21**. Every former “1.0” acceptance criterion is mapped to delivered
children or an **accepted residual** for this cut. Human GO is still required
at tag time (automation cannot self-approve).

## Children of JOE-1655

All direct children are **Done** (QE, supply chain, platforms, threat model,
RC freeze/dogfood/rollback/exit, external-review templates). Related epics also
**Done**: JOE-1913 (external review findings), JOE-1974 (post-provider security
freeze), JOE-1920 (human sign-off), JOE-2212 (remote STT chunk-and-stitch),
JOE-2213 (catalogue probe).

## Acceptance map (honest)

### Continuous quality and adversarial testing

| Criterion | Disposition | Evidence |
|-----------|-------------|---------|
| cargo-fuzz targets (parsers, DTOs, cleanup, …) | **Met** | JOE-1861 / JOE-1884; CI fuzz-smoke + scheduled campaigns |
| PR + scheduled fuzz limits / triage | **Met** | `fuzz-campaign.yml`, `docs/operations/fuzzing.md` |
| Miri pure suite | **Met** | JOE-1889; CI miri job |
| ASan / concurrency stress | **Met** (gaps accepted) | JOE-1887; Linux ASan + stress; macOS/Windows ASan + full UBSan = residual |
| Trust-boundary coverage policy | **Met** | JOE-1888; `coverage_trust.sh` |
| Mutation semantics | **Met** | JOE-1886; smoke + `mutation_semantics` kill list |
| Deterministic fault injection | **Met** | foundation + governor/runtime tests; CI coverage |

**Accepted residual (QE):** UBSan full matrix and full whisper/ORT under ASan
remain gaps (documented in `qe-depth.md`). Not release-blocking for 0.0.21.

### Dependency and artifact supply chain

| Criterion | Disposition | Evidence |
|-----------|-------------|---------|
| RustSec / cargo-deny / tool pins | **Met** | CI security; `check_security_tool_pins.sh` |
| Native inventory | **Met** | JOE-1902; SBOM native-components |
| Formal SBOM per release artifact | **Met** | JOE-1859; CycloneDX/SPDX generate + verify |
| Cosign keyless / checksums | **Met** | JOE-1882; release.yml + verify |
| Provenance bind to full SHA | **Met** | JOE-1860 / F-006; PROVENANCE + verify |
| Independent clean verify job | **Met** | JOE-1891; `release-verify.yml` |
| Model revocation rehearsal | **Met** | JOE-1892; scripts |

**Accepted residual (F-006):** full SLSA attestations per artifact and crates.io
package provenance parity are **not claimed**. Dual-builder bit-for-bit equality
not claimed where variance is documented (`reproducibility.md`).

### Reproducibility and platforms

| Criterion | Disposition | Evidence |
|-----------|-------------|---------|
| Tier matrix | **Met** | JOE-1863; `platform-support.md` |
| Two-builder variance report | **Met** | JOE-1885; CI repro-smoke / release compare |
| Clean-install Tier A | **Met** | JOE-1883; CI clean-install matrix |
| Doctor fail-closed on unsupported | **Met** | doctor / platform docs |

### Threat model and independent review

| Criterion | Disposition | Evidence |
|-----------|-------------|---------|
| Threat-model matrix | **Met** | JOE-1893 |
| Multi-tenant unsupported | **Met** | explicit residual F-004 |
| Independent review + retest | **Met** | JOE-1913 / F-001–F-006; JOE-1920 human sign-off on v0.0.19 freeze |
| Disclosure tabletop | **Met** | JOE-1890 |

**Accepted residual (post-provider):** OpenRouter TTS remains **experimental**
until protected live smoke (JOE-1978 residual). Catalogue probe (JOE-2213)
prevents shipping dead defaults without demotion.

### Compatibility freeze and RC programme

| Criterion | Disposition | Evidence |
|-----------|-------------|---------|
| Freeze inventory + automated check | **Met** | JOE-1896; `rc_freeze_check.sh` green on tip |
| Downstream consumer gate | **Met** | JOE-1903 |
| Dogfood checklist + automation | **Met** | JOE-1897; CI rc-dogfood-smoke |
| Support / security-fix policy | **Met** | JOE-1898 |
| Rollback rehearsal | **Met** | JOE-1895 |
| RC exit report generator | **Met** | JOE-1904; retargeted to v0.0.21 language |
| Human GO | **Pending at cut** | required at tag; not self-declared |

**Accepted residual (RC process):** multi-day multi-platform human dogfood is
represented by automated Tier A clean-install + dogfood smoke + maintainer
sign-off at cut, not a separate multi-week freeze calendar.

## Product follow-ups closed into this tip

| Issue | Status |
|-------|--------|
| JOE-2212 remote STT chunk-and-stitch | Done (PR #85) |
| JOE-2213 provider catalogue probe | Done (PR #86) |
| Docs/skills 0.0.21 framing | Done (PR #84) |

## Machine evidence at closeout

Run on tip `b9af171…` (0.0.20 workspace):

```text
rc_freeze_check.sh OK
generate_rc_exit_report.sh OK  (freeze/sbom/downstream/rollback pass)
```

Regenerate at cut time from the release candidate SHA:

```bash
./scripts/rc_freeze_check.sh
./scripts/generate_rc_exit_report.sh dist/rc-exit
./scripts/probe_provider_catalogues.sh --offline
```

## Programme decision

**JOE-1655 is complete for shipping v0.0.21** subject to:

1. Human sign-off on the RC exit report for the **tag commit**
2. Acceptance of residuals listed above (F-006, QE platform gaps, OpenRouter TTS experimental, no multi-day freeze calendar)

No open child of JOE-1655 blocks the cut. Residual work after v0.0.21 continues
as ordinary 0.0.x issues (e.g. OpenRouter TTS promotion, UBSan matrix).

## Related

* [rc-exit.md](rc-exit.md) · [release-gate.md](release-gate.md) · [qe-depth.md](qe-depth.md)
* [external-review-disposition-2026-08-01.md](external-review-disposition-2026-08-01.md)
* [external-review-disposition-2026-08-02.md](external-review-disposition-2026-08-02.md)
* [post-provider-security-freeze.md](post-provider-security-freeze.md)
