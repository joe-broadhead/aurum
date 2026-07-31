# Release

!!! important "No surprise releases"
    Do **not** tag or publish without explicit maintainer approval.

## Versioning

Semantic versioning. Source of truth: **`VERSION`**.

Must match:

- workspace `version` in root `Cargo.toml`
- `## [x.y.z]` in `CHANGELOG.md`
- crate versions via `version.workspace = true` (`aurum-core`, `aurum-stt`, `aurum-ffi`)

```bash
./scripts/version_check.sh
```

## Checklist before prepare

Prefer the automated gate (JOE-1640):

```bash
./scripts/release_gate.sh
```

Manual equivalent:

1. `VERSION` set to intended release  
2. `CHANGELOG.md` has `## [x.y.z] - YYYY-MM-DD` (nothing important left only under Unreleased)  
3. `cargo test --workspace --locked`  
4. `cargo clippy --workspace --all-targets --locked -- -D warnings`  
5. `cargo check -p aurum-core --no-default-features --locked`  
6. `mkdocs build --strict`  
7. `cargo audit` + `cargo deny check`  
8. `./scripts/generate_sbom.sh dist/sbom`  
9. Optional: `AURUM_INTEGRATION=1 cargo test -p aurum-core --test local_integration -- --ignored`  
10. Optional: `./scripts/publish_dry_run.sh`  

See also [release gate](../operations/release-gate.md) and [supply chain](supply-chain.md).

## Flow

```text
1. workflow_dispatch → Prepare Release (version=x.y.z)
2. Merge release/x.y.z PR into master
3. release-tag creates vX.Y.Z (after version_check)
4. release.yml: fail-closed tag checkout → test → audit/deny → SBOM
   → platform CLI binaries → SHA256SUMS + PROVENANCE + GitHub Release
```

## Manual tag (fallback)

```bash
git tag -a v0.0.2 -m "Release v0.0.2"
git push origin v0.0.2
```

## Assets (CLI)

| Asset | Platform / purpose |
|-------|--------------------|
| `aurum-macos-arm64` | Apple Silicon (Tier A) |
| *(Intel Mac)* | Build from source (`cargo install aurum-stt`) — Tier B |
| `aurum-linux-x86_64` | Linux GNU (Tier A) |
| `aurum-windows-x86_64.exe` | Windows MSVC (Tier A) |
| `SHA256SUMS` | Checksums for all attached assets |
| `PROVENANCE.txt` | Tag, source commit, workflow run metadata |
| `aurum-sbom-inventory.json` / `.md` | Rust dependency inventory (JOE-1635) |
| `native-components.md` | whisper / ort / ffmpeg notes |
| `cargo-metadata.json` | Full Cargo graph snapshot |

Verify downloads:

```bash
./scripts/verify_release_assets.sh /path/to/downloaded-assets
```

`aurum-ffi` is **not** attached as a prebuilt dylib in v0.0.x — consumers build from source (`cargo build -p aurum-ffi --release`).

## crates.io (optional, separate from GitHub Release)

Not triggered by tags. Prefer GitHub Release binaries for end-user CLI installs.

**Local:**

```bash
./scripts/publish_dry_run.sh
cargo publish -p aurum-core --locked   # first
cargo publish -p aurum-stt --locked    # after core is on the index
# aurum-ffi: publish when you want the crate on crates.io (optional)
cargo publish -p aurum-ffi --locked
```

**CI (manual workflow):** Actions → **Publish crates.io**

1. Secret `CARGO_REGISTRY_TOKEN` (repo or `crates-io` environment)
2. Dry-run then publish: **`aurum-core` → `aurum-stt`** (order matters)
3. `aurum-ffi` only if intentionally publishing the FFI crate

The bare name `aurum` is taken on crates.io (unrelated AUR helper) — CLI package is **`aurum-stt`**.
