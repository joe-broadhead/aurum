# Credential hygiene (JOE-1715)

## crates.io publishing token

Release publication uses a **GitHub Actions secret** (`CARGO_REGISTRY_TOKEN` or
equivalent) in the protected release/publish environment.

### Rotation checklist

1. **Revoke** any token that was pasted into chat, email, CI logs, or a ticket.
   On crates.io: Account → API Tokens → revoke the compromised token id.
2. **Create** a replacement token with the minimum practical scope (publish only
   for the `aurum-*` crates you own).
3. **Store** the replacement only in the protected GitHub environment secret.
   Never commit it, paste it into Linear/GitHub issues, or echo it in shell
   history you will retain.
4. **Inventory** repository and organisation secrets for stale copies.
5. **Prove** the old token no longer authenticates (negative check / crates.io
   audit log).
6. **Review** publication history since disclosure for unexpected crates or
   versions.
7. Record a **redacted** incident note (token id prefix, timestamps only).

### Fail-closed security tools

CI installs **exact** versions of `cargo-audit` and `cargo-deny` with
`--locked --version …`. There is **no** fallback to an unreviewed latest
install. `scripts/check_security_tool_pins.sh` enforces this on workflow YAML.

### What this issue does not cover

Formal keyless publishing, Sigstore/cosign, and SLSA attestation remain in the
v0.0.21 production-assurance programme (formerly labelled “1.0”).
