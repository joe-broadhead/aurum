# Supply chain (JOE-1634 / JOE-1635)

## GitHub Actions

Third-party Actions are pinned to **full commit SHAs** with a comment naming the
intended upstream tag (JOE-1634). Dependabot/manual bumps must update both.

Least privilege:

- Default `permissions: contents: read`
- Write permissions only on publish/tag jobs

Tag checkout is **fail-closed**: release builds check out the exact tag object,
not mutable branch HEADs.

## SBOMs and provenance

```bash
./scripts/generate_sbom.sh dist/sbom
```

Outputs:

| File | Purpose |
|------|---------|
| `cargo-metadata.json` | Full Cargo graph |
| `aurum-sbom-inventory.json` | Versioned package inventory |
| `aurum-sbom-inventory.md` | Human table |
| `native-components.md` | whisper/ort/ffmpeg notes |

Release assets also include:

- `SHA256SUMS` for all binaries/docs attached
- `PROVENANCE.txt` with tag, commit, timestamp

Verify downloads:

```bash
./scripts/verify_release_assets.sh /path/to/downloaded-assets
```

## Dependency policy

- `deny.toml` — licenses, sources, advisories (`cargo deny check`)
- `cargo audit` — RustSec lockfile advisories

Exceptions require owner + rationale in `deny.toml` (never silent).

## Signing / attestations

GitHub Release artifacts ship with SHA-256 checksums. Optional future work:
Sigstore/cosign attestations and SLSA provenance. Until then, operators must
verify checksums against the release page over HTTPS.
