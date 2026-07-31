# Named-hardware performance reports (JOE-1739)

## Purpose

Retain **real model** latency, RTF, memory, and concurrency evidence for release
claims. PR smoke benches remain pure-Rust hot paths (`scripts/bench_smoke.sh`).

## Required fields

Each retained report JSON should include:

- commit / version / rustc / target / features
- hardware profile id (`apple_silicon_metal`, `linux_x86_cpu`, `constrained`, …)
- OS and machine name (non-secret)
- model id + artifact digest when available
- cold vs warm
- end-to-end vs inference-only
- RTF, p50, p95 wall ms
- peak RSS when available
- concurrency level and queue wait
- repetitions / notes on noise

## Local capture

```bash
# PR-safe microbenches
./scripts/bench_smoke.sh

# Full matrix (pre-cached models only; operator-run)
./scripts/run_perf_report.sh --profile apple_silicon_metal --model tiny-q5_1
```

Reports are written under `evals/reports/` (gitignored samples) or an operator
archive. Do not commit private machine serials or user audio paths.
