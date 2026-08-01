# RC multi-platform dogfood checklist (JOE-1897)

**Document version:** 1.0  
**Tier A platforms:** macOS arm64, Linux x86_64, Windows x86_64  
See [platform-support.md](platform-support.md).

## Purpose

During a 1.0 RC interval, operators run this checklist on **every Tier A
platform** (or CI automates the green cells). Results fill the evidence
template for the RC exit report.

## Automation

```bash
# Automated subset (doctor, cache, freeze check, clean-install smoke when possible)
./scripts/rc_dogfood_checklist.sh --tag v0.0.16 --out dist/rc-dogfood

# CI job `rc-dogfood-smoke` runs the automated subset on Linux PR/master.
```

Manual cells remain operator-owned (real STT/TTS listening, remote OpenRouter).

## Checklist matrix

Legend: **A** = automatable · **M** = manual · **B** = both

| # | Item | Mode | Pass criteria |
|---|------|------|---------------|
| 1 | Clean install (binary or source) | B | `clean_install_smoke` exit 0 |
| 2 | Release asset verify (cosign) | A | `independent_release_verify` or `verify_release_assets` |
| 3 | `aurum --version` / doctor offline | A | exit 0, no secrets in JSON |
| 4 | Cache status + verify | A | no unexpected network; quarantine works |
| 5 | Cold STT (tiny model, local) | M | transcript non-empty |
| 6 | Warm STT (repeat same model) | M | faster path, still correct |
| 7 | Offline STT after cache filled | M | works with `local_only` / offline |
| 8 | Local TTS smoke | M | WAV written, duration > 0 |
| 9 | Rules cleanup | A/M | fillers stripped |
| 10 | Remote OpenRouter STT (optional) | M | only with key; redacted logs |
| 11 | Concurrent FFI/jobs stress | A | `stress` unit tests |
| 12 | Cache repair after quarantine | M | re-download or repair restores trust |
| 13 | Fault: missing ffmpeg message | M | doctor fail-closed guidance |
| 14 | Upgrade from previous tag | M | install next tag, doctor green |
| 15 | Rollback note (docs only) | M | know supersede-tag procedure |
| 16 | Freeze inventory check | A | `rc_freeze_check.sh` |

## Evidence template (per platform)

Copy to `dist/rc-dogfood/<platform>-evidence.md`:

```markdown
# RC dogfood evidence

- platform:
- tag:
- operator:
- date_utc:
- install_method: binary | source
- freeze_check: pass | fail
- clean_install: pass | fail
- verify_assets: pass | fail | n/a
- doctor_offline: pass | fail
- cold_stt: pass | fail | skipped
- warm_stt: pass | fail | skipped
- offline_stt: pass | fail | skipped
- tts: pass | fail | skipped
- cleanup: pass | fail | skipped
- remote: pass | fail | skipped
- concurrent_jobs: pass | fail
- notes:
- residual_risks:
```

## Exit criteria for RC

- All **A** rows green on CI for Linux; macOS/Windows covered by CI matrix jobs
  where present (test, clean-install) plus operator manual M rows.
- No unresolved Critical regression against frozen surfaces ([rc-freeze.md](rc-freeze.md)).
- Security issues only via [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md).

## Related

* [rc-freeze.md](rc-freeze.md) · [rc-rollback.md](rc-rollback.md) · [release-gate.md](release-gate.md)
