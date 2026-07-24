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

## From crates.io

```bash
cargo install aurum-stt
aurum --version
```

Package name is `aurum-stt` (the `aurum` crate name is already taken on crates.io). The installed binary is still **`aurum`**.

Library:

```toml
aurum-core = "0.0.0"
```

## From source

```bash
git clone https://github.com/joe-broadhead/aurum.git
cd aurum
./scripts/install.sh
# equivalent: cargo install --path crates/aurum --locked --force
aurum --version
```

## From GitHub Releases

After a tagged release, download the binary for your platform from
[Releases](https://github.com/joe-broadhead/aurum/releases), verify `SHA256SUMS`,
and place it on your `PATH`:

```bash
chmod +x aurum-macos-arm64
sudo mv aurum-macos-arm64 /usr/local/bin/aurum
aurum --version
```

| Asset | Platform |
|-------|----------|
| `aurum-macos-arm64` | Apple Silicon |
| `aurum-macos-x86_64` | Intel Mac |
| `aurum-linux-x86_64` | Linux GNU |
| `aurum-windows-x86_64.exe` | Windows |

## Library only (`aurum-core`)

You do not need the CLI binary to use the library. See
[Library integration](../library/integration.md).
