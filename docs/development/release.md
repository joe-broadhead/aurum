# Release

!!! important "No surprise releases"
    Do **not** tag or publish without explicit maintainer approval.

## Versioning

Semantic versioning. Source of truth: **`VERSION`**.

Must match:

- workspace `version` in root `Cargo.toml`
- `## [x.y.z]` in `CHANGELOG.md`
- crate versions via `version.workspace = true`

```bash
./scripts/version_check.sh
```

## Checklist before prepare

1. `VERSION` set to intended release  
2. `CHANGELOG.md` has `## [x.y.z] - YYYY-MM-DD` (nothing important left only under Unreleased)  
3. `cargo test --workspace --locked`  
4. `cargo clippy --workspace --all-targets --locked -- -D warnings`  
5. `mkdocs build --strict`  
6. Optional: `AURUM_INTEGRATION=1 cargo test -p aurum-core --test local_integration -- --ignored`  
7. Optional: `./scripts/publish_dry_run.sh`  

## Flow

```text
1. workflow_dispatch → Prepare Release (version=0.0.0)
2. Merge release/0.0.0 PR into main
3. release-tag creates v0.0.0 (after version_check)
4. release.yml builds platform binaries + SHA256SUMS + GitHub Release
```

## Manual tag (fallback)

```bash
git tag -a v0.0.0 -m "Release v0.0.0"
git push origin v0.0.0
```

## Assets

| Asset | Platform |
|-------|----------|
| `aurum-macos-arm64` | Apple Silicon |
| `aurum-macos-x86_64` | Intel Mac |
| `aurum-linux-x86_64` | Linux GNU |
| `aurum-windows-x86_64.exe` | Windows |
| `SHA256SUMS` | Checksums |

## crates.io (optional, separate from GitHub Release)

Not triggered by tags. Prefer GitHub Release binaries for the CLI.

**Local:**

```bash
./scripts/publish_dry_run.sh
cargo publish -p aurum-core          # first
cargo publish -p aurum               # only after core is on the index
```

**CI (manual workflow):** Actions → **Publish crates.io**

1. Repo secret or environment `crates-io` secret: `CARGO_REGISTRY_TOKEN`
2. Dry-run `aurum-core` → real publish `aurum-core` → optional `aurum`

`aurum` dry-run / publish fails until `aurum-core` exists on crates.io — expected.
