# Release provenance and verification (JOE-1860)

Aurum release assets ship with **byte digests**, **formal SBOMs**, and a
**structured provenance record**. Cryptographic attestation (cosign keyless)
is optional but supported when operators configure identity env vars.

## Artifacts in a GitHub Release

| File | Role |
|------|------|
| `aurum-macos-arm64`, `aurum-linux-x86_64`, `aurum-windows-x86_64.exe` | Tier A CLI binaries |
| `SHA256SUMS` | Digests of all release files (including SBOMs + provenance) |
| `aurum.cdx.json` | CycloneDX 1.5 SBOM (required) |
| `aurum.spdx.json` | SPDX 2.3 lite SBOM (required) |
| `aurum-sbom-inventory.json` / `.md` | Human-oriented dependency inventory |
| `SBOM_MANIFEST.json` | SHA-256 of formal SBOM files |
| `PROVENANCE.json` | Structured build provenance (tag, commit, workflow, per-asset digests) |
| `PROVENANCE.txt` | Human-readable summary of the same |
| `SHA256SUMS.bundle` (optional) | cosign keyless signature bundle |

## Verify a downloaded release (fail-closed)

```bash
# After extracting or downloading all assets into ./release-assets
export AURUM_EXPECT_TAG=v0.0.11
export AURUM_EXPECT_COMMIT=<full git sha of the tag>
./scripts/verify_release_assets.sh ./release-assets
```

What the script checks:

1. Every line in `SHA256SUMS` matches on-disk bytes.
2. Formal SBOMs parse and contain required schema fields + non-empty package lists.
3. When `PROVENANCE.json` is present, tag/commit match env expectations and
   listed asset digests match files.

### Optional cosign verify

If the release includes `SHA256SUMS.bundle` and you have `cosign` installed:

```bash
export AURUM_COSIGN_CERTIFICATE_IDENTITY='https://github.com/joe-broadhead/aurum/.github/workflows/release.yml@refs/tags/v0.0.11'
export AURUM_COSIGN_CERTIFICATE_OIDC_ISSUER='https://token.actions.githubusercontent.com'
./scripts/verify_release_assets.sh ./release-assets
```

Identity and issuer values are **release-specific**. Document the current
production identity in the release notes when signing is enabled.

## Generating provenance locally

```bash
./scripts/generate_sbom.sh dist/sbom
mkdir -p dist/release-assets
cp dist/sbom/* dist/release-assets/
# copy binaries into dist/release-assets, then:
./scripts/generate_provenance.sh dist/release-assets v0.0.11
# rebuild SHA256SUMS after provenance is written (see release.yml)
```

## Trust boundaries

* **Checksums** prove byte equality with the published set — not who published them.
* **PROVENANCE.json** binds the asset set to a git tag/commit and CI run id when
  produced by `release.yml`.
* **cosign keyless** (when enabled) binds the checksum file to a GitHub OIDC
  identity for the release workflow. Rotation/revocation follows GitHub's
  keyless transparency log; compromised tokens are addressed by rotating
  workflow permissions and publishing a superseding tag.

## Related

* [release-gate.md](release-gate.md) — gates before tag
* [platform-support.md](platform-support.md) — Tier A/B/C platforms
* [security.md](security.md) — security process
