# Release

!!! important "No surprise releases"
    Do **not** tag or publish without explicit maintainer approval.

## Versioning

Semantic versioning. Public preview line is `0.x.y`. Source of truth: **`VERSION`**.

Must match:

- workspace `version` in root `Cargo.toml`
- `## [x.y.z]` in `CHANGELOG.md`
- crate versions via `version.workspace = true`

```bash
./scripts/version_check.sh
```

## Checklist before prepare

1. `VERSION` set to intended release  
2. `CHANGELOG.md` has `## [x.y.z] - YYYY-MM-DD` (move notes out of Unreleased)  
3. `cargo test --workspace --locked`  
4. `cargo clippy --workspace --all-targets --locked -- -D warnings`  
5. `mkdocs build --strict`  
6. Integration test optional: `AURUM_INTEGRATION=1 cargo test -p aurum-core --test local_integration -- --ignored`

## Flow (same shape as ZephyrFlow / dbt-nova)

```text
1. workflow_dispatch → Prepare Release (version=0.0.0)
2. Merge release/0.0.0 PR into main
3. release-tag creates v0.0.0
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

## crates.io

**Not automated in v0.0.0.** When approved:

```bash
cargo publish -p aurum-core --dry-run
cargo publish -p aurum-core
# CLI binary package is optional on crates.io; prefer GitHub Release binaries
```

Consumers should prefer **git tags/revs** until the API stabilizes at `0.1.0`.
