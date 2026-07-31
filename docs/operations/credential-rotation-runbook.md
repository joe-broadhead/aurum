# crates.io credential rotation runbook (JOE-1715)

Complete this **after** any publish that still used the disclosed token (v0.0.4
crates are published). Do **not** paste token values into chat, tickets, or git.

## Steps

1. Open https://crates.io/settings/tokens while logged into the publisher account.
2. Identify the token used for the v0.0.3 chat paste / any long-lived publish token.
3. **Revoke** that token.
4. Create a new token:
   * Name: `aurum-github-actions-publish` (or similar)
   * Scopes: minimum needed to publish `aurum-core`, `aurum-stt`, `aurum-ffi`
5. In GitHub → repo **Settings → Environments → `crates-io`** (or org secrets):
   * Set `CARGO_REGISTRY_TOKEN` to the **new** value only
   * Remove any duplicate org/repo copies of the old token
6. Negative check (old token must fail):
   ```bash
   # Do not put tokens in shell history — use a one-off env in a private session
   CARGO_REGISTRY_TOKEN='OLD_REVOKED' cargo publish -p aurum-core --dry-run
   # expect authentication failure
   ```
7. Positive check (new token, dry-run only):
   ```bash
   # Via GitHub Actions: workflow_dispatch crates-publish.yml dry_run=true
   ```
8. Review crates.io account “Activity” / crate publish history since disclosure.
9. Record redacted note (token name + revoke timestamp only) in a private ops log.

## Already done in v0.0.4 code

* Fail-closed pinned `cargo-audit` / `cargo-deny` installs
* `scripts/check_security_tool_pins.sh`

## Done when

* Old token revoked and proven unusable
* New token only in protected environment secret
* Activity review complete with no unexplained publishes
