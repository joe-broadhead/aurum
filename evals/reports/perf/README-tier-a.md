# Tier A performance field reports (JOE-2317)

Evidence version: `0.0.22-perf-v1`  
Schema: 2 (`PerfReport`)  
**Disposition:** three-platform field evidence retained — programme residual **Done**
with documented GHA limits (not full scenario catalogue).

## Retained reports

| Profile | File | Hardware (coarse) | Scenarios |
|---------|------|-------------------|-----------|
| Maintainer macOS arm64 | `perf-tier_a_macos_arm64-field.json` | Apple M4, 10c/16 GiB, macOS 26.6 | doctor, CLI STT, 30s STT, Kitten TTS |
| GHA Linux x86_64 | `perf-tier_a_linux_x86_64_gnu-gha.json` | AMD EPYC 7763, 4c/15 GiB, Ubuntu 24.04 | doctor, CLI STT, 30s STT, Kitten TTS |
| GHA macOS arm64 | `perf-tier_a_macos_arm64_gha-gha.json` | Apple M1 Virtual, 3c/7 GiB, macOS 14.8 | doctor, CLI STT, 30s STT, Kitten TTS |
| GHA Windows x86_64 | `perf-tier_a_windows_x86_64_msvc-gha.json` | AMD64 Family 25, 4c/15 GiB, Win 10.0.26100 | doctor, CLI STT |

Budget seeds: `evals/observatory/budgets/perf-tier_a_*.field.json`

## Headline numbers (informational across machines)

| Scenario | Maintainer macOS | GHA Linux | GHA macOS | GHA Windows |
|----------|------------------|-----------|-----------|-------------|
| doctor_startup p50 | ~6.5 ms | ~3.6 ms | ~8.4 ms | ~31 ms |
| cli_stt_one_file p50 | ~257 ms | ~1141 ms | ~1492 ms | ~13315 ms |
| stt 30s warm p50 | ~488 ms | ~4990 ms | ~5350 ms | — |
| kitten short TTS p50 | ~821 ms | ~1892 ms | ~1407 ms | — |

## Honesty

* GHA numbers are **named-hardware family** evidence, not maintainer desktops.
* Cross-platform p50 comparison is informational and never hides same-machine regression.
* Full scenario catalogue (concurrency, batch_20, large models) is **not** claimed.
* Windows full re-run (30883816912) hit `Illegal instruction` on STT warm after
  toolchain rebuild; retained Windows report is from prior successful GHA capture
  (30863347968). Selective `platforms=windows` re-dispatch supported.
* 30s STT uses looped short fixture content (duration proxy, not unique speech).

## Sources

* Maintainer capture: `scripts/eval/run_tier_a_perf_capture.py` local
* GHA full (Linux+macOS TTS): https://github.com/joe-broadhead/aurum/actions/runs/30883816912
* GHA Windows (prior): https://github.com/joe-broadhead/aurum/actions/runs/30863347968
* Workflow: `.github/workflows/tier-a-perf-capture.yml`
