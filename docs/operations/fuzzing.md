# Continuous fuzzing (JOE-1861 / JOE-1884)

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
| `wav_parse` | Direct WAV/hound load (`try_load_wav_file`) with tight limits |
| `ffi_validators` | Pure FFI `CleanupStyle` / `JobState` mapping + `cleanup_rules` |

All targets use `aurum-core` / `aurum-ffi` with **`default-features = false`**
(no ORT) so fuzz workers stay free of local TTS native packs.

## Local usage

```bash
# One-time
rustup toolchain install nightly
cargo +stable install cargo-fuzz --locked --version 0.12.0

cd fuzz
cargo +nightly fuzz run config_toml -- -max_total_time=60
cargo +nightly fuzz run wav_parse -- -max_total_time=60
cargo +nightly fuzz run ffi_validators -- -max_total_time=60
```

### Crash artifacts and triage

Crashes land under `fuzz/artifacts/<target>/`. Workflow:

1. Minimize: `cargo +nightly fuzz tmin <target> fuzz/artifacts/<target>/crash-*`
2. Reproduce: `cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-*`
3. File an issue with the **minimized** input (or a unit test seed under
   `fuzz/corpus/<target>/` once fixed).
4. Security-sensitive crashes: follow root `SECURITY.md` — do not open a public
   issue with a weaponized PoC until fixed or coordinated disclosure.

CI uploads crash artifacts from scheduled campaigns when present
(`.github/workflows/fuzz-campaign.yml`).

## CI

* **PR smoke** — `ci.yml` job `fuzz-smoke` runs each target for ~20s on nightly.
* **Scheduled long campaigns** — `fuzz-campaign.yml` (cron + `workflow_dispatch`)
  runs multi-hour, resource-capped campaigns (`-max_total_time` per target,
  RSS cap). Failures fail the job; crash dirs are uploaded as artifacts.
* Corpus growth is not committed by default; seeds can be added under
  `fuzz/corpus/<target>/` when a regression must stay fixed.

## Embargo

Security-relevant crashes (memory corruption, auth bypass, secret leak) use the
private disclosure path in the repository `SECURITY.md`. Public regression
tests land **after** a fix is on `master` or with a coordinated disclosure date.
