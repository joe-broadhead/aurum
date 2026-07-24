# CI

Workflows live under `.github/workflows/`.

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | PR + push to main | fmt, clippy `-D warnings`, tests, multi-OS build, MSRV, docs strict, version sync |
| `docs.yml` | docs paths / main | MkDocs build + GitHub Pages deploy |
| `release-prepare.yml` | manual | Cut `release/x.y.z` PR |
| `release-tag.yml` | merge release PR | Create `vX.Y.Z` tag + dispatch release |
| `release.yml` | tag `v*` | Multi-platform CLI binaries + checksums |

## Local parity

```bash
make ci
```
