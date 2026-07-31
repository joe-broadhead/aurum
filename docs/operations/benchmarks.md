# Benchmark harness and performance budgets (JOE-1606)

## Profiles

| Profile id | Hardware |
|------------|----------|
| `apple_silicon_metal` | Apple Silicon macOS + Metal whisper |
| `linux_x86_cpu` | x86_64 Linux CPU |
| `constrained` | Low-memory / mobile defaults |

## Smoke (PR-safe)

No unpinned model downloads. Measures pure-Rust hot paths:

```bash
cargo test -p aurum-core bench -- --nocapture
./scripts/bench_smoke.sh
```

JSON schema (`BenchReport`, `schema_version = 1`) includes commit, target,
features, warm/cold flag, and samples with p50/p95 wall ms + optional peak RSS.

## Full release matrix (scheduled / local)

Requires pre-cached models (`local_only` / already on disk):

| Scenario | Metrics |
|----------|---------|
| Cold/warm STT load | wall, peak RSS |
| Local STT by model/length | RTF, p50/p95, peak RSS |
| Local TTS | load, time-to-first-audio, RTF, peak RSS |
| Concurrency 1/2/4/8 | throughput, p95, queue wait, threads |
| Partial STT | revision latency, duplicate rate |
| Remote path | encode time, wire bytes, peak working set |
| Cleanup / output / FFI / shutdown | overhead |

## Regression policy

- Smoke budgets are soft and profile-scaled (see `SmokeBudgets` in `aurum_core::bench`).
- Full baselines are reviewed per release; fail only on **material** p95/RSS
  regressions after noise-aware repeats — not single-shot noise.
- Never download unpinned models during a benchmark.
