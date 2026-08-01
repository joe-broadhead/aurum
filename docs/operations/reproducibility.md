# Build reproducibility and variance (JOE-1885)

Aurum records **measured variance** between isolated builders for the same
git tag/commit. Full bit-for-bit reproducibility is a stretch goal; operators
must still verify `SHA256SUMS` + cosign for the **published** asset set.

## Two-builder comparison

```bash
# Build twice into isolated CARGO_TARGET_DIR trees and write VARIANCE_REPORT.md
./scripts/compare_release_builds.sh --target x86_64-unknown-linux-gnu

# Or rebuild against a published baseline binary:
./scripts/compare_release_builds.sh \
  --baseline ./aurum-linux-x86_64 \
  --target x86_64-unknown-linux-gnu \
  --out dist/repro
```

CI:

* **PR / master** — `ci.yml` job `repro-smoke` runs a dual Linux rebuild (no
  strict digest match required).
* **Release** — `release.yml` job `repro-compare` rebuilds linux-x86_64 against
  the published artifact and uploads `VARIANCE_REPORT.md` as a workflow artifact.

Set `AURUM_REPRO_STRICT=1` to fail when digests differ (useful for experiments
with fully deterministic toolchains; **not** the default release gate).

## Expected variance vs hard mismatch

| Class | Examples | Gate |
|-------|----------|------|
| **Expected variance** | path embeds, PE timestamps, macOS UUID, linker version drift | Report only |
| **Hard mismatch** | missing binary, size ratio >2×, `--version` differs | Fail script / CI |

## Trust model

1. **Published** digests in `SHA256SUMS` (cosign-signed) are authoritative for
   downloaders.
2. Variance reports explain whether a second builder can recreate the same
   bytes — they do **not** replace signature verification.
3. Toward 1.0, reduce variance with stripped release profiles and pinned
   toolchains where practical.

## Related

* [provenance.md](provenance.md) — checksums, SBOM, cosign
* [platform-support.md](platform-support.md) — Tier A binaries
* [release-gate.md](release-gate.md)
