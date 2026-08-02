# Threat model and control matrix (JOE-1637 / JOE-1893)

**Document version:** 2.0 (Group J)  
**Applies to:** Aurum v0.0.x toward the **v0.0.21** production-assurance cut  
**Owner:** maintainer (joe-broadhead/aurum)

This document supersedes the v1 profile sketch with a **versioned control
matrix**: High/Critical threats map to implementation controls, verification
evidence, and residual-risk dispositions.

## 1. Assets

| Asset | Sensitivity | Notes |
|-------|-------------|-------|
| User audio / transcripts / TTS text | **Critical** | PII / confidential content |
| API keys (`OPENROUTER_API_KEY`) | **Critical** | Remote path only |
| Model/voice caches | **High** | Integrity of native execution |
| Output files (transcripts, WAV) | **High** | User data at rest |
| Release binaries / crates / SBOMs | **Critical** | Supply chain |
| C ABI / host process | **High** | FFI embeds |

## 2. Actors

| Actor | Capability |
|-------|------------|
| Local user | Full CLI; trusted OS account in single-user profile |
| Host app / agent | May multiplex untrusted inputs; may be multi-tenant |
| Remote OpenRouter | Only when explicitly selected |
| Network attacker | MITM, malicious model hosts, release mirror tampering |
| Malicious local FS | Symlink races, hostile media, poisoned cache |
| Compromised CI token | Attempt to publish bad release assets |

## 3. Trust boundaries (data flow summary)

```text
[mic/file] → ffmpeg/hound decode → PCM bounds → local whisper OR remote OpenRouter
                                                      ↓
                                              cleanup (rules|remote)
                                                      ↓
                                              output transaction → disk
[host FFI] → aurum-ffi Engine/jobs → same core path (no nested host Tokio)
[cache] ← model pins (SHA-256 + exact size) ← HF download only on miss
[release] GitHub OIDC cosign → SHA256SUMS.bundle + SBOM + PROVENANCE
```

| Boundary | Rule |
|----------|------|
| Process | Other processes on a shared host are untrusted |
| Network | Default local path does not open sockets (except explicit model download) |
| Filesystem | Output transaction rejects symlink clobber; model packs verify digests |
| Native code | whisper.cpp / ORT are code-adjacent trust |
| FFI | Host must honor free/ownership; invalid pointers are not fully defensively safe |
| Supply chain | Tag checkout fail-closed; Action SHA pins; cosign keyless required on publish |

## 4. Deployment profiles

| Profile | Status | Assumptions | Required controls |
|---------|--------|-------------|-------------------|
| **A — Single-user local CLI** | **Supported** | Trusted OS account | Local provider default; cache integrity; no secrets in logs |
| **B — Desktop / mobile embed** | **Supported** | Untrusted media/text | Input size caps; cancel; no payload logging; local_only |
| **C — Native plugin / multi-caller** | **Supported** | Concurrent hosts | Engine isolation; job API; ResourceGovernor |
| **D — Custom endpoint / BYOM** | **Supported with caution** | Operator accepts third-party model risk | Pins; `trust=verified` preferred; `local_unverified` explicit |
| **E — Server / multi-tenant** | **Unsupported as product guarantee** | Hostile tenants | **Outer sandbox required**; no shared cache identity; document residual |

### Server / multi-tenant disposition (explicit)

Aurum does **not** provide in-process tenant isolation. Operating multi-tenant
without an outer sandbox is **unsupported**. Residual risk is accepted only when
the host documents an OS-level sandbox, per-tenant cache dirs, and network
policy. See control **T-MT-01**.

## 5. Control matrix (High / Critical)

Legend: **Sev** C=Critical H=High · **Disp** Mitigated / Residual / Unsupported

| ID | Threat | Sev | Profiles | Control (implementation) | Verification evidence | Residual / disposition |
|----|--------|-----|----------|--------------------------|----------------------|------------------------|
| T-NET-01 | Unexpected network on local path | C | A–C | Default `local` provider; OpenRouter only when selected | Integration + doctor offline; code review remote module | Mitigated |
| T-KEY-01 | API key leakage in logs/errors | C | D | Secret redaction; `Debug` redaction tests | `secret` unit tests; adversarial secret tests | Mitigated |
| T-DL-01 | Poisoned model download | C | A–D | Reviewed SHA-256 + exact size pins; verify-before-publish | `pinned_sha256` catalogue tests; cache verify | Mitigated for catalogue; Residual for unpinned BYOM |
| T-CACHE-01 | Tampered cache artifact | H | A–D | `aurum cache verify` quarantines bad digests | cache unit tests; CLI verify | Mitigated |
| T-FS-01 | Symlink / clobber races on output | H | A–C | Output transaction noclobber + symlink reject | fault_injection / transaction tests | Mitigated |
| T-AUD-01 | Hostile media → native decoder crash | H | A–C | FFmpeg supervised limits; size/duration caps; fuzz WAV | fuzz `wav_parse`; audio unit tests | Residual: native ffmpeg/whisper bugs |
| T-FFI-01 | UAF / nested runtime from host | H | B–C | Jobs API; engine pools; shutdown drain | FFI ABI tests; stress concurrent jobs | Residual: host misuse of raw C pointers |
| T-GOV-01 | Resource exhaustion (jobs/models) | H | B–C | ResourceGovernor + job concurrency caps | fault injection; stress tests | Mitigated for documented limits |
| T-SUP-01 | Compromised release binary | C | all | Cosign keyless `SHA256SUMS.bundle`; formal SBOM; PROVENANCE | `verify_release_assets.sh`; independent verify workflow | Mitigated when operators verify cosign |
| T-SUP-02 | Mutable CI Action supply chain | H | all | Action SHA pin policy | `check_action_pins.sh` | Mitigated |
| T-BYOM-01 | Malicious custom ONNX pack | H | D | `trust` modes; digests; no bare onnx path | TTS BYOM docs; pack validation | Residual: `local_unverified` is operator risk |
| T-MT-01 | Cross-tenant data mix | C | E | **Unsupported** without outer sandbox | Docs only | **Unsupported** — host must isolate |
| T-REV-01 | Known-bad model pin remains trusted | H | A–D | Pin revocation runbook + superseding release | `rehearse_model_revocation.sh` | Mitigated via process |
| T-DISC-01 | Public issue before fix | H | all | SECURITY.md private path + tabletop | disclosure evidence pack | Mitigated process |

## 6. Medium threats (summary)

| ID | Threat | Control | Disposition |
|----|--------|---------|-------------|
| T-LOG-01 | Path leakage in support bundles | Redaction in support/doctor | Mitigated |
| T-CFG-01 | Hostile config TOML | Fail-closed parse + size caps | Mitigated |
| T-DEP-01 | RustSec dependency vuln | cargo audit + deny on PR | Mitigated |

## 7. Verification map (evidence owners)

| Evidence | Location |
|----------|----------|
| Fuzz / Miri / mutation / coverage | [qe-depth.md](qe-depth.md), [fuzzing.md](fuzzing.md) |
| Release cosign / SBOM | [provenance.md](provenance.md) |
| Independent download verify | `.github/workflows/release-verify.yml` |
| Model pin revoke | [model-revocation.md](model-revocation.md) |
| Disclosure tabletop | [disclosure-tabletop.md](disclosure-tabletop.md) |
| Platform tiers | [platform-support.md](platform-support.md) |

## 8. Non-goals

- Multi-tenant isolation inside a single process without host cooperation
- Making arbitrary ONNX execution “safe”
- Replacing an outer OS sandbox for hostile multi-tenant media
- Real-time response SLA beyond “best effort working day” acknowledgement

## Related

* [security.md](security.md) · [hardening.md](hardening.md) · [release-gate.md](release-gate.md)
* Root [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md)
