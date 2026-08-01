# Continuous fuzzing (JOE-1861)

Aurum keeps **two layers** of adversarial testing:

1. **PR-stable** — `crates/aurum-core/tests/adversarial_parsers.rs` and
   `fault_injection.rs` on stable Rust (every CI run).
2. **libFuzzer / cargo-fuzz** — `fuzz/` directory (this document). Nightly +
   short PR smoke; longer scheduled campaigns.

## Targets

| Target | Boundary |
|--------|----------|
| `config_toml` | Config TOML load |
| `stt_dto_json` | STT DTO deserialize + `try_from_dto` |
| `segment_validate` | Segment construction / validation |
| `rules_cleanup` | Rules cleanup styles |
| `output_format` | Format parse + TXT/JSON/SRT emit |

All targets use `aurum-core` with **`default-features = false`** (no ORT) so
fuzz workers stay pure-Rust.

## Local usage

```bash
# One-time
rustup toolchain install nightly
cargo install cargo-fuzz --locked

cd fuzz
cargo +nightly fuzz run config_toml -- -max_total_time=60
cargo +nightly fuzz run stt_dto_json -- -max_total_time=60
```

Crashes land under `fuzz/artifacts/<target>/`. Minimize and file an issue; if
the crash is security-sensitive, follow the repository root `SECURITY.md`
disclosure process and do not open a public issue with a weaponized PoC until
fixed or disclosed.

## CI

* **PR smoke** — `ci.yml` job `fuzz-smoke` runs each target for ~20s on nightly.
* Failures (crash/timeout hang) fail the job.
* Corpus growth is not committed by default; seeds can be added under
  `fuzz/corpus/<target>/` when a regression must stay fixed.

## Embargo

Security-relevant crashes (memory corruption, auth bypass, secret leak) use the
private disclosure path in the repository `SECURITY.md`. Public regression
tests land **after** a fix is on `master` or with a coordinated disclosure date.
