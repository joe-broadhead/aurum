# Installation

## Prerequisites

| Tool | Why |
|------|-----|
| **Rust 1.89+** | MSRV / build from source |
| **cmake** + C/C++ toolchain | Builds `whisper-rs` → whisper.cpp |
| **ffmpeg** | Decode mp3, m4a, and other non-16 kHz mono WAVs (system install, not bundled) |

```bash
# macOS
brew install rustup-init cmake ffmpeg
rustup default stable

# Ubuntu / Debian
sudo apt install build-essential cmake pkg-config ffmpeg
# install rustup from https://rustup.rs
```

## Recommended: verified release binary

```bash
# One-shot (curl) — installs latest GitHub Release binary after SHA256 verify
curl -fsSL https://raw.githubusercontent.com/joe-broadhead/aurum/master/scripts/install.sh \
  | bash -s -- --from-release

# Or from a clone:
./scripts/install.sh --from-release
AURUM_VERSION=v0.0.21 ./scripts/install.sh --from-release
```

Default install directory: `~/.local/bin` (`AURUM_INSTALL_DIR` to override).

```bash
./scripts/install.sh --uninstall   # removes binary only; keeps cache + config
```

| Asset | Platform |
|-------|----------|
| `aurum-macos-arm64` | Apple Silicon |
| `aurum-macos-x86_64` | *(not shipped — build from source; see note)* |
| `aurum-linux-x86_64` | Linux GNU |
| `aurum-windows-x86_64.exe` | Windows |

## From crates.io

```bash
cargo install aurum-stt --locked
aurum --version
```

Package name is `aurum-stt` (the `aurum` crate name is already taken on crates.io). The installed binary is still **`aurum`**.

Library:

```toml
aurum-core = "0.0.21"
```

## From source

```bash
git clone https://github.com/joe-broadhead/aurum.git
cd aurum
./scripts/install.sh --from-source
# equivalent: cargo install --path crates/aurum --locked --force
aurum --version
```

## Shell completions and man page

```bash
aurum completions bash > /usr/local/etc/bash_completion.d/aurum
aurum completions zsh  > ~/.zfunc/_aurum
aurum completions fish > ~/.config/fish/completions/aurum.fish
aurum completions powershell > aurum.ps1
aurum man | sudo tee /usr/local/share/man/man1/aurum.1 >/dev/null
```

## Intel Mac (x86_64)

Prebuilt **Intel Mac** binaries are not published (ONNX Runtime has no cross-compile
prebuilts for `x86_64-apple-darwin` from CI Apple Silicon runners). On Intel Mac:

```bash
cargo install aurum-stt --locked
# or: cargo install --path crates/aurum --locked
```

Apple Silicon uses the `aurum-macos-arm64` release asset.

## Library only (`aurum-core`)

You do not need the CLI binary to use the library. See
[Library integration](../library/integration.md).

## Native library (`aurum-ffi`)

For C/Swift/Kotlin embeds, build the FFI crate (not needed for CLI users):

```bash
cargo build -p aurum-ffi --release
```

See [Native embeds](../library/ffi.md).
