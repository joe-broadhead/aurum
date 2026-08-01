# Model-manifest revocation and security updates (JOE-1892)

When a reviewed STT/TTS pin is compromised, wrong, or withdrawn upstream, Aurum
**fails closed** on the trusted path. This runbook covers revocation, superseding
releases, and user notification.

## Principles

1. **Pins are identity** — catalogue filenames map to immutable SHA-256 + exact size.
2. **Never silent re-pin** — changing a pin is a security-relevant release.
3. **Quarantine, do not auto-delete** — `aurum cache verify` moves bad artifacts aside for forensics.
4. **Supersede, do not rewrite tags** — publish `vX.Y.Z+1` (or next 0.0.x); do not force-push release tags.

## When to revoke

* Digest mismatch vs reviewed pin (upstream rewrote a file)
* Security advisory on a model weight / pack
* Accidental pin of non-reviewed artifact
* Operator decision to withdraw a catalogue entry

## Procedure (production)

1. **Triage** — confirm filename, expected pin, observed digest, severity.
2. **Stop the trusted path** — remove or comment the pin in `pinned_sha256` /
   `pinned_exact_bytes` (STT) or TTS pack catalogue so `verify` / download fail closed.
3. **Quarantine local caches** (operators):
   ```bash
   aurum cache verify
   # inspect cache/quarantine/
   ```
4. **Ship a security release**:
   * Bump VERSION + CHANGELOG (Security section)
   * `cargo test --workspace --locked` + `./scripts/release_gate.sh`
   * Tag and publish (SBOM + cosign automatic in `release.yml`)
5. **Advisory** — GitHub Security Advisory or public security note; link superseding tag.
6. **Notify** — release notes must name revoked filename(s) and required action
   (`cache verify`, re-download from new pin only).
7. **Verify** — independent `release-verify` workflow + local
   `./scripts/verify_release_assets.sh` with `AURUM_REQUIRE_COSIGN=1`.

## Dry-run / rehearsal

```bash
# Does not modify production pins; exercises checklist + pin inventory.
./scripts/rehearse_model_revocation.sh
```

Exit 0 means the rehearsal completed and produced an evidence stub under
`dist/security-rehearsal/`.

## Evidence template (RC exit)

| Field | Value |
|-------|-------|
| Date (UTC) | |
| Operator | |
| Filename(s) revoked | |
| Old pin (sha256) | |
| Superseding tag | |
| Advisory URL | |
| Independent verify run URL | |
| Residual risk | |

## Related

* [threat-model.md](threat-model.md) control **T-REV-01**
* [provenance.md](provenance.md) · [security.md](security.md)
* Root [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md)
