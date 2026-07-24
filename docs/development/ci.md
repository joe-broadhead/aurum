# CI

Workflows live under `.github/workflows/`.

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | PR + push to main | fmt, clippy `-D warnings`, tests (3 OS), MSRV 1.89, docs strict, version sync, macOS integration |
| `docs.yml` | docs paths / main | MkDocs build + GitHub Pages deploy |
| `release-prepare.yml` | manual | Cut `release/x.y.z` PR |
| `release-tag.yml` | merge release PR | Create `vX.Y.Z` after version_check + dispatch release |
| `release.yml` | tag `v*` | Multi-platform CLI binaries + SHA256SUMS |

## Local parity

```bash
make ci
./scripts/version_check.sh
.venv/bin/mkdocs build --strict
```

## Branch protection

`main` requires status checks: **Lint**, **test (ubuntu-22.04)**, **Docs**, **msrv (1.89)**. Force-push disabled.
