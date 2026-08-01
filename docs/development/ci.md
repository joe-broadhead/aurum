# CI

Workflows live under `.github/workflows/`.

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | PR + push to master | fmt, clippy `-D warnings`, tests (ubuntu-24.04 / macOS / Windows), MSRV 1.89, STT-only, security (audit+deny), docs strict, version sync, action pins, macOS integration, fuzz smoke, clean-install matrix, repro smoke, **Miri / sanitizer-stress / trust coverage / mutants smoke** (JOE-1886–1889) |
| `docs.yml` | docs paths / master | MkDocs build + GitHub Pages deploy |
| `fuzz-campaign.yml` | schedule + manual | Long cargo-fuzz campaigns (JOE-1884) |
| `release-verify.yml` | schedule + release published + manual | Independent download+cosign verify (JOE-1891) + security rehearsal |

RC freeze (JOE-1896–1898): `ci.yml` jobs `rc-freeze-check` and `rc-dogfood-smoke`
run freeze inventory tests, dogfood automated subset, and rollback rehearsal.
| `release-prepare.yml` | manual | Cut `release/x.y.z` PR |
| `release-tag.yml` | merge release PR | Create `vX.Y.Z` after version_check + dispatch release |
| `release.yml` | tag `v*` or manual | Fail-closed tag checkout, security gate, multi-platform **CLI** binaries + SBOM + SHA256SUMS + cosign + PROVENANCE + GitHub Release |
| `crates-publish.yml` | **manual only** | crates.io dry-run or publish (`aurum-core`, then `aurum-stt`, optional `aurum-ffi`) |

Workspace members covered by CI: **`aurum-core`**, **`aurum-stt`**, **`aurum-ffi`**.

### Security & quality jobs (JOE-1578)

| Job | What |
|-----|------|
| `security` | `cargo audit` + `cargo deny check` (`deny.toml`) |
| `stt-only` | `aurum-core --no-default-features` + adversarial/fault tests |
| `lint` | also runs `scripts/check_action_pins.sh` (SHA-pinned Actions) |

Third-party Actions are pinned to **full commit SHAs** with a nearby tag comment
(JOE-1634). See [supply-chain.md](supply-chain.md).

Adversarial suites (every PR via workspace test + explicit stt-only job):

```bash
cargo test -p aurum-core --test adversarial_parsers --locked
cargo test -p aurum-core --test fault_injection --locked
```

QE depth (JOE-1886–1889) — see [qe-depth.md](../operations/qe-depth.md):

```bash
./scripts/run_miri.sh
./scripts/run_sanitizers.sh --stress
./scripts/coverage_trust.sh
./scripts/run_mutants.sh --smoke
```

## Local parity

```bash
make ci
./scripts/version_check.sh
./scripts/check_action_pins.sh
./scripts/publish_dry_run.sh
cargo test -p aurum-ffi --locked
cargo deny check   # requires cargo-deny
cargo audit        # requires cargo-audit
# optional network/cache integration:
cargo test -p aurum-core --test local_integration -- --ignored
cargo test -p aurum-core --test tts_synth -- --ignored
# full pre-tag gate:
./scripts/release_gate.sh
.venv/bin/mkdocs build --strict
```

## Branch protection

`master` requires status checks: **Lint**, **test (ubuntu-24.04)**, **Docs**, **msrv (1.89)**.
Prefer also requiring **Security (audit + deny)** and **STT-only** once green on master.
Force-push disabled.

## crates.io

Not part of the GitHub Release tag flow. When approved:

1. Optional environment **`crates-io`**
2. Secret **`CARGO_REGISTRY_TOKEN`**
3. Actions → **Publish crates.io** (dry-run first)

CLI binaries for end users come from **GitHub Releases**. Library consumers use crates.io **`aurum-core`** / **`aurum-stt`**. Native embeds build **`aurum-ffi`** from source unless/until that crate is published.
