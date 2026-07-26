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
aurum-core = "0.0.2"
```

## From source

```bash
git clone https://github.com/joe-broadhead/aurum.git
cd aurum
./scripts/install.sh
# equivalent: cargo install --path crates/aurum --locked --force
aurum --version
```

## Intel Mac (x86_64)

Prebuilt **Intel Mac** binaries are not published (ONNX Runtime has no cross-compile
prebuilts for `x86_64-apple-darwin` from CI Apple Silicon runners). On Intel Mac:

```bash
cargo install aurum-stt --locked
# or: cargo install --path crates/aurum --locked
```

Apple Silicon uses the `aurum-macos-arm64` release asset.

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
| `aurum-macos-x86_64` | *(not shipped — build from source; see note)* |
| `aurum-linux-x86_64` | Linux GNU |
| `aurum-windows-x86_64.exe` | Windows |

## Library only (`aurum-core`)

You do not need the CLI binary to use the library. See
[Library integration](../library/integration.md).

## Native library (`aurum-ffi`)

For C/Swift/Kotlin embeds, build the FFI crate (not needed for CLI users):

```bash
cargo build -p aurum-ffi --release
```

See [Native embeds](../library/ffi.md).
