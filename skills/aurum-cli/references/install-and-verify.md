# Install and verify

## Preferred: GitHub Release binary

```bash
# From a clone:
./scripts/install.sh --from-release
# Or pin:
AURUM_VERSION=v0.0.20 ./scripts/install.sh --from-release
```

The installer downloads the platform asset, verifies `SHA256SUMS`, and installs to
`$AURUM_INSTALL_DIR` (default `~/.local/bin`).

## crates.io

```bash
cargo install aurum-stt --locked
aurum --version
```

Package name is `aurum-stt`; binary name is `aurum`.

## Source

```bash
./scripts/install.sh --from-source
# requires rustc 1.89+, cmake, ffmpeg recommended
```

## Completions and man page

```bash
aurum completions zsh > ~/.zfunc/_aurum
aurum man | sudo tee /usr/local/share/man/man1/aurum.1
```

## Uninstall

```bash
./scripts/install.sh --uninstall
# preserves cache (~/.cache/aurum) and config
```

## Verify

```bash
aurum doctor
aurum models
aurum tts models
```

Speech workflows: load `skills/aurum-speech/`.
