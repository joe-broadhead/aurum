# Installation

## Prerequisites

| Tool | Why |
|------|-----|
| **Rust 1.89+** | Build from source / MSRV |
| **cmake** + C/C++ toolchain | Builds `whisper-rs` → whisper.cpp |
| **ffmpeg** | Decode mp3/m4a/… (system install, not bundled) |

```bash
# macOS
brew install rustup cmake ffmpeg
rustup default stable

# Ubuntu/Debian
sudo apt install build-essential cmake pkg-config ffmpeg
# install rustup from https://rustup.rs
```

## From source (recommended while private)

```bash
git clone https://github.com/joe-broadhead/aurum.git
cd aurum
./scripts/install.sh
# or: cargo install --path crates/aurum --locked --force
aurum --version
```

## From GitHub Releases (when published)

After a tagged release, download the asset for your platform from
[Releases](https://github.com/joe-broadhead/aurum/releases), verify `SHA256SUMS`,
make the binary executable, and place it on your `PATH`.

```bash
# example shape (names match release assets)
chmod +x aurum-macos-arm64
sudo mv aurum-macos-arm64 /usr/local/bin/aurum
```

## Library only (`aurum-core`)

See [Library integration](../library/integration.md). You do not need the CLI
binary to depend on the core crate.
