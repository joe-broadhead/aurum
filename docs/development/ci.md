# CI

Workflows live under `.github/workflows/`.

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | PR + push to master | fmt, clippy `-D warnings`, tests (ubuntu-24.04 / macOS / Windows), MSRV 1.89, docs strict, version sync, macOS integration |
| `docs.yml` | docs paths / master | MkDocs build + GitHub Pages deploy |
| `release-prepare.yml` | manual | Cut `release/x.y.z` PR |
| `release-tag.yml` | merge release PR | Create `vX.Y.Z` after version_check + dispatch release |
| `release.yml` | tag `v*` or manual | Multi-platform **CLI** binaries + SHA256SUMS + GitHub Release |
| `crates-publish.yml` | **manual only** | crates.io dry-run or publish (`aurum-core`, then `aurum-stt`, optional `aurum-ffi`) |

Workspace members covered by CI: **`aurum-core`**, **`aurum-stt`**, **`aurum-ffi`**.

## Local parity

```bash
make ci
./scripts/version_check.sh
./scripts/publish_dry_run.sh
cargo test -p aurum-ffi --locked
.venv/bin/mkdocs build --strict
```

## Branch protection

`master` requires status checks: **Lint**, **test (ubuntu-24.04)**, **Docs**, **msrv (1.89)**. Force-push disabled.

## crates.io

Not part of the GitHub Release tag flow. When approved:

1. Optional environment **`crates-io`**
2. Secret **`CARGO_REGISTRY_TOKEN`**
3. Actions → **Publish crates.io** (dry-run first)

CLI binaries for end users come from **GitHub Releases**. Library consumers use crates.io **`aurum-core`** / **`aurum-stt`**. Native embeds build **`aurum-ffi`** from source unless/until that crate is published.
