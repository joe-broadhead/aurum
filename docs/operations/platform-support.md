# Platform support matrix (JOE-1863)

Aurum ships continuous **0.0.x** releases. Tiers describe what we ship and test — not a frozen major
guarantee. Use this matrix for install planning and clean-install qualification.

## Tiers

| Tier | Platforms | Install method | Evidence |
|------|-----------|----------------|----------|
| **A — binary** | macOS arm64 (Apple Silicon), Linux x86_64 (gnu), Windows x86_64 (MSVC) | GitHub Release CLI + `SHA256SUMS` | Built in `release.yml`; integration smoke on macOS arm64 in CI |
| **B — source** | macOS x86_64 (Intel), other Linux glibc hosts | `cargo install aurum-stt --locked` or `./scripts/install.sh --from-source` | Compile on stable MSRV; no prebuilt ONNX for Intel Mac cross-compile |
| **C — experimental** | Other targets / musl / freeBSD | `cargo build` best-effort | Community; no release gate |

### System dependencies

| Dependency | Required for | Notes |
|------------|--------------|-------|
| **ffmpeg** | STT decode of non-WAV formats | Must be on `PATH`; not bundled |
| **cmake** / C toolchain | Building from source | whisper.cpp / native deps |
| **disk + cache dir** | Model packs | Writable `AURUM_CACHE` / default cache |

### Feature notes

* Default CLI builds include **local TTS** (`ort` download-binaries).
* STT-only: `cargo build -p aurum-core --no-default-features` (CI job).
* Remote STT/cleanup needs network + `OPENROUTER_API_KEY` (explicit opt-in).

## Clean-install qualification

Use the smoke script on a fresh machine or empty cache:

```bash
# From a release checkout or cloned tag:
./scripts/clean_install_smoke.sh --from-release --version v0.0.20

# Or source build (Tier B / CI):
./scripts/clean_install_smoke.sh --from-source
```

The script checks:

1. Install succeeds into a temp `AURUM_INSTALL_DIR`
2. `aurum --version` / `aurum doctor` (offline) exit 0
3. Release mode verifies `SHA256SUMS` when downloading
4. Uninstall removes the binary only (cache/config preserved)

### CI matrix (JOE-1883)

`ci.yml` job **`clean-install`** runs `--from-source` on Tier A runners:

| Runner | Maps to |
|--------|---------|
| `ubuntu-24.04` | linux-x86_64 |
| `macos-14` | macos-arm64 |
| `windows-latest` | windows-x86_64 |

Failures are red CI and block release readiness ([release-gate.md](release-gate.md)).
Scheduled release-download checks can use `--from-release` against a published tag.

## Unsupported environments

Doctor and preflight should **fail closed** with actionable text when:

* ffmpeg is missing (STT non-WAV)
* cache is not writable
* model missing under `--local-only`
* OpenRouter used offline / without key

Do not crash with an uncaught panic for these cases.

## Related

* [installation.md](../getting-started/installation.md)
* [provenance.md](provenance.md) — verify release assets
* [release-gate.md](release-gate.md)
