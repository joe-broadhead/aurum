# Named-hardware performance reports (JOE-1739 / JOE-2218)

Evidence version: **`0.0.22-perf-v1`**  
Code: `aurum_core::eval::perf` (`PerfReport`, `compare_perf_budget`)

## Purpose

Measure complete user-visible workflows on **named Tier A machines** with
explicit regression budgets. Compiler flags and microbenchmarks alone are not
product evidence.

## Tier A hardware families

Retain at least one report for each:

1. **macOS arm64** — exact Apple chip, core count, memory, OS version  
2. **Linux x86_64 GNU** — exact CPU, cores, memory, kernel/distribution  
3. **Windows x86_64 MSVC** — exact CPU, cores, memory, OS build  

Hardware identifiers are coarse product specs — never device serials or user
names. Templates: `tier_a_profile_templates()`.

## Scenario catalogue

Stable IDs from `perf_scenario_catalogue()` cover:

* Local STT: cold load, warm 30s / 5m / long-form, concurrency 1/2/4 for
  `tiny-q5_1`, `base`, profile models, `large-v3-turbo`
* Local TTS: Kitten and Kokoro short / paragraph / multi-chunk
* Workflows: one-file CLI STT, batch ≥20, doctor startup, C ABI overhead,
  long-form mock remote
* Remote: informational only (never blocks local release readiness)

Release-gated scenarios are marked `release_gated: true` in the catalogue.

## Statistics

* ≥5 measured repetitions for expensive real-model scenarios  
* ≥20 repetitions for sub-second / product-overhead scenarios  
* Warmups excluded from reported samples  
* Report **median (p50)** and **p95**, not only the fastest run  
* Separate download/network time from local inference  
* Separate queue wait from execution  

## Regression policy (same machine + model digest)

| Check | Threshold |
|-------|-----------|
| p50 wall or RTF | **>10%** → **warn** (review) |
| p95 wall | **>15%** → **fail** unless baseline approved |
| peak RSS | **>15%** or **+256 MiB** (whichever larger) → **fail** |
| concurrency throughput | drop **>15%** → **fail** |
| governor overload | must reject/queue per config, not exceed ceiling |

Baseline updates require before/after report, explanation, and changelog.
Cross-machine comparisons are informational and never hide a same-machine
regression.

## Artifacts

| Path | Role |
|------|------|
| `evals/reports/perf/` | Retained named-hardware reports |
| `evals/observatory/budgets/perf-*.json` | Committed budgets |
| `scripts/eval/compare_perf_budget.py` | Fail-closed compare |
| `scripts/run_perf_report.sh` | Operator capture helper |
| `scripts/bench_smoke.sh` | PR-safe pure-Rust microbenches |

## Commands

```bash
# PR-safe microbenches
./scripts/bench_smoke.sh

# Operator capture (pre-cached models only)
./scripts/run_perf_report.sh --profile apple_silicon_metal --model tiny-q5_1

# Budget compare (non-zero on fail)
python3 scripts/eval/compare_perf_budget.py \
  --report evals/reports/perf/perf-apple_silicon_metal-tiny-q5_1.json \
  --budget evals/observatory/budgets/perf-tier_a_macos_arm64.example.json

cargo test -p aurum-core --lib percentile
cargo test -p aurum-core --lib budget_pass_and_p95
```

## Privacy

Reports contain scenario IDs, durations, byte counts, and model/provider IDs
only — never transcript text, input audio, synthesis text, keys, private voice
IDs, or raw remote responses.

## Related

* JOE-2216 STT quality (RTF cross-link fields)
* JOE-2217 TTS objective wall time
* JOE-2222 privacy-safe metrics/traces
* Historical evidence: [evidence-v004.md](evidence-v004.md)
