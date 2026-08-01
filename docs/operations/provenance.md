# Release provenance and verification (JOE-1860 / JOE-1882)

Aurum release assets ship with **byte digests**, **formal SBOMs**, a
**structured provenance record**, and a **required cosign keyless** signature
bundle over `SHA256SUMS` (JOE-1882).

## Artifacts in a GitHub Release

| File | Role |
|------|------|
| `aurum-macos-arm64`, `aurum-linux-x86_64`, `aurum-windows-x86_64.exe` | Tier A CLI binaries |
| `SHA256SUMS` | Digests of all release files (including SBOMs + provenance) |
| `SHA256SUMS.bundle` | **Required** cosign keyless signature bundle over `SHA256SUMS` |
| `aurum.cdx.json` | CycloneDX 1.5 SBOM (required) |
| `aurum.spdx.json` | SPDX 2.3 lite SBOM (required) |
| `aurum-sbom-inventory.json` / `.md` | Human-oriented dependency inventory |
| `SBOM_MANIFEST.json` | SHA-256 of formal SBOM files |
| `PROVENANCE.json` | Structured build provenance (tag, commit, workflow, per-asset digests) |
| `PROVENANCE.txt` | Human-readable summary of the same |

## Verify a downloaded release (fail-closed)

```bash
# After downloading all assets into ./release-assets
export AURUM_EXPECT_TAG=v0.0.13
export AURUM_EXPECT_COMMIT=<full git sha of the tag>
# Cosign identity for the release workflow + tag (see table below)
export AURUM_REQUIRE_COSIGN=1
export AURUM_COSIGN_CERTIFICATE_OIDC_ISSUER='https://token.actions.githubusercontent.com'
export AURUM_COSIGN_CERTIFICATE_IDENTITY="https://github.com/joe-broadhead/aurum/.github/workflows/release.yml@refs/tags/${AURUM_EXPECT_TAG}"
# Or accept any release tag from this workflow:
# export AURUM_COSIGN_CERTIFICATE_IDENTITY_REGEXP='https://github.com/joe-broadhead/aurum/\.github/workflows/release\.yml@refs/tags/v[0-9]+\.[0-9]+\.[0-9]+'
./scripts/verify_release_assets.sh ./release-assets
```

What the script checks:

1. Every line in `SHA256SUMS` matches on-disk bytes.
2. Formal SBOMs parse and contain required schema fields + non-empty package lists.
3. When `PROVENANCE.json` is present, tag/commit match env expectations and
   listed asset digests match files.
4. When `AURUM_REQUIRE_COSIGN=1` (or a bundle is present with identity env),
   `cosign verify-blob` validates `SHA256SUMS.bundle` against the OIDC identity.

Install cosign: <https://docs.sigstore.dev/cosign/system_config/installation/>

### Production identity (current)

| Field | Value |
|-------|--------|
| OIDC issuer | `https://token.actions.githubusercontent.com` |
| Certificate identity | `https://github.com/joe-broadhead/aurum/.github/workflows/release.yml@refs/tags/<tag>` |
| Signed artifact | `SHA256SUMS` (covers all other assets via digests) |
| Bundle file | `SHA256SUMS.bundle` |

### Rotation and revocation

* **Rotation:** Keyless signing uses short-lived GitHub OIDC tokens; there is no
  long-lived release private key to rotate. Changing the signing workflow path
  or repository name produces a **new** certificate identity — update this
  table and release notes, and verify with the new identity string.
* **Revocation / compromise response:** If a workflow or token is suspected
  compromised: revoke GitHub Actions secrets, lock down workflow permissions,
  publish a superseding tag with a clean run, and instruct users to distrust
  the prior tag's assets. Fulcio/Rekor transparency logs retain historical
  signatures for audit.
* **Offline dry-runs:** Local `generate_provenance.sh` runs do not produce a
  cosign bundle. Set `AURUM_REQUIRE_COSIGN=0` (default) for local asset checks.

## Generating provenance locally

```bash
./scripts/generate_sbom.sh dist/sbom
mkdir -p dist/release-assets
cp dist/sbom/* dist/release-assets/
# copy binaries into dist/release-assets, then:
./scripts/generate_provenance.sh dist/release-assets v0.0.13
# rebuild SHA256SUMS after provenance is written (see release.yml)
# cosign sign-blob is performed only in GitHub Actions OIDC context
```

## Trust boundaries

* **Checksums** prove byte equality with the published set — not who published them.
* **PROVENANCE.json** binds the asset set to a git tag/commit and CI run id when
  produced by `release.yml`.
* **cosign keyless** binds the checksum file to a GitHub OIDC identity for the
  release workflow at that tag. Always prefer `AURUM_REQUIRE_COSIGN=1` for
  production download verification.

## Related

* [release-gate.md](release-gate.md) — gates before tag
* [reproducibility.md](reproducibility.md) — two-builder variance
* [platform-support.md](platform-support.md) — Tier A/B/C platforms
* [security.md](security.md) — security process
