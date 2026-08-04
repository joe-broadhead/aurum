# Tier A performance field reports (JOE-2317)

Evidence version: `0.0.22-perf-v1`  
Schema: 2 (`PerfReport`)

## Retained reports

| Profile | File | Hardware (coarse) | Scenarios |
|---------|------|-------------------|-----------|
| Maintainer macOS arm64 | `perf-tier_a_macos_arm64-field.json` | Apple M4, 10 cores, 16 GiB, macOS 26.6 | doctor, CLI STT, 30s STT, Kitten short TTS |
| GHA Linux x86_64 | `perf-tier_a_linux_x86_64_gnu-gha.json` | AMD EPYC 7763, 4 cores, 15 GiB, Ubuntu 24.04 | doctor, CLI STT, 30s STT |
| GHA Windows x86_64 | `perf-tier_a_windows_x86_64_msvc-gha.json` | AMD64 Family 25, 4 cores, 15 GiB, Windows 10.0.26100 | doctor, CLI STT |

Budget seeds (same-machine regression only):  
`evals/observatory/budgets/perf-tier_a_*.field.json`

## Honesty

* GHA numbers are **named-hardware family** evidence, not maintainer desktops.
* Cross-platform p50 comparison is informational and never hides same-machine regression.
* Kitten TTS omitted on GHA when the pack is not pre-cached (`--local-only`).
* Windows 30s STT omitted in run `30863347968` (ffmpeg missing; workflow now installs choco ffmpeg).
* macOS GHA job failed on empty-array `set -u` expansion; maintainer macOS report already retained. Workflow fixed.

## Source

GHA: https://github.com/joe-broadhead/aurum/actions/runs/30863347968  
Capture: `scripts/eval/run_tier_a_perf_capture.py`  
Workflow: `.github/workflows/tier-a-perf-capture.yml`
