# QE depth: Miri, sanitizers, coverage, mutation (JOE-1886–1889)

Aurum **v0.0.x** continuous quality has four layers beyond PR unit tests and
cargo-fuzz ([fuzzing.md](fuzzing.md)):

| Layer | Script | CI | Issue |
|-------|--------|-----|-------|
| **Miri** (pure Rust) | `scripts/run_miri.sh` | `ci.yml` `miri` | JOE-1889 |
| **Sanitizers + stress** | `scripts/run_sanitizers.sh` | `ci.yml` `sanitizer-stress` | JOE-1887 |
| **Trust coverage** | `scripts/coverage_trust.sh` | `ci.yml` `coverage-trust` | JOE-1888 |
| **Mutation** | `scripts/run_mutants.sh` | `ci.yml` `mutants-smoke` + scheduled optional | JOE-1886 |

## Miri (JOE-1889)

```bash
./scripts/run_miri.sh
```

Runs **curated** unit-test filters under `cargo +nightly miri test` on
`aurum-core --no-default-features` with `MIRIFLAGS=-Zmiri-disable-isolation`
(tempfile/mkdir for output transaction tests):

* `domain::`, `dto::`, `output::tests::` (formatters; not filesystem publish)
* `secret::`, `providers::tests::` (segment / DTO validation)
* model digest pin unit tests (`model::tests::reviewed…`)
* best-effort pure `JobState` mapping under `aurum-ffi` when linkable

### Explicit Miri gaps (not silent)

| Code path | Why skipped | Residual risk owner |
|-----------|-------------|---------------------|
| whisper.cpp / whisper-rs FFI | Foreign functions unsupported | Integration tests + release binaries |
| ORT / local TTS | Native + feature-gated | TTS integration (`--ignored`) |
| Tokio async I/O / FFmpeg / cleanup rules | `kqueue`/`epoll` foreign ops | `fault_injection` + native unit tests |
| `output::transaction` publish | `hard_link` unsupported in Miri | output unit tests on native |
| Full process CLI | Not a lib unit surface | clean-install + integration-local |

Failures in the curated set **fail CI**.

## Sanitizers and concurrency stress (JOE-1887)

```bash
./scripts/run_sanitizers.sh          # ASan on Linux when available + stress
./scripts/run_sanitizers.sh --stress # concurrency only
```

| Combination | Status |
|-------------|--------|
| ASan + pure lib tests on **Linux nightly** | Enabled in CI |
| ASan on macOS/Windows hosts | **Gap** — toolchain/friction; covered by Linux CI |
| UBSan full matrix | **Accepted residual for v0.0.21** — ASan + Miri first; track later 0.0.x |
| Full whisper/ORT under ASan | **Gap** — link time / false positives; integration suite instead |
| Concurrency stress (jobs / fault injection) | Enabled on stable in CI |

## Branch coverage policy (JOE-1888)

```bash
./scripts/coverage_trust.sh dist/coverage-trust
```

Produces `TRUST_COVERAGE.md` with **per-module** line/branch stats for trust
boundaries (domain, dto, output, cleanup, providers, model digests, error,
secret, cancel, runtime, audio parse). CI uploads the report as an artifact.

### Soft floors (0.0.x trend — not vanity PR blockers)

| Module class | Line | Branch (when measured) | Before v0.0.21 RC |
|--------------|-----:|------------------------:|---------------|
| domain / dto / output / cleanup rules | ≥70% | ≥50% | add tests if below |
| model digest pins / error mapping | ≥80% | ≥60% | **RC exit blocker** if below |
| providers segment validation | ≥70% | ≥50% | add adversarial seeds |
| native STT/TTS | n/a | n/a | integration evidence |

**Do not** treat a single workspace-wide coverage percentage as the quality
signal.

## Mutation testing (JOE-1886)

```bash
./scripts/run_mutants.sh --smoke   # PR: sharded, time-capped
./scripts/run_mutants.sh --full    # local / scheduled
```

Scoped to: `domain`, `dto`, `output`, `cleanup/rules`, `error`, `providers/mod`,
`model/mod` under `--no-default-features`.

### Survivor policy

* **Must kill** before v0.0.21 RC: digest mismatch acceptance, bounds bypass,
  invalid lifecycle transitions, capability/error mapping flips that weaken
  fail-closed behavior.
* **Often acceptable**: `Display`/`Debug` formatting, pure logging branches.
* Survivors are listed in `dist/mutants/MUTANTS_SUMMARY.md` for human review.

## Related

* [fuzzing.md](fuzzing.md) — libFuzzer campaigns
* [release-gate.md](release-gate.md) — pre-tag gates
* [ci.md](../development/ci.md) — workflow map
