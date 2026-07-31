#!/usr/bin/env bash
# Aurum installer (JOE-1720).
#
# Modes:
#   --from-source   Build from this repo (default when run inside a checkout)
#   --from-release  Download a verified GitHub Release binary
#   --upgrade       Same as --from-release for the latest tag (or --version)
#   --uninstall     Remove the installed binary only (preserves cache/config)
#
# Environment:
#   AURUM_INSTALL_DIR   default: $HOME/.local/bin
#   AURUM_REPO          default: joe-broadhead/aurum
#   AURUM_VERSION       optional tag (e.g. v0.0.4); default: latest release
set -euo pipefail

REPO="${AURUM_REPO:-joe-broadhead/aurum}"
INSTALL_DIR="${AURUM_INSTALL_DIR:-${HOME}/.local/bin}"
VERSION="${AURUM_VERSION:-}"
MODE="auto"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

usage() {
  cat <<'EOF'
Usage: install.sh [--from-source|--from-release|--upgrade|--uninstall] [options]

  --from-source     cargo install from this repository
  --from-release    download GitHub Release binary + verify SHA256SUMS
  --upgrade         alias for --from-release (latest or AURUM_VERSION)
  --uninstall       remove $AURUM_INSTALL_DIR/aurum (keeps cache + config)
  --version TAG     pin release tag (e.g. v0.0.4)
  --install-dir DIR override install directory (default: ~/.local/bin)
  -h, --help        show this help

Examples:
  ./scripts/install.sh --from-source
  curl -fsSL https://raw.githubusercontent.com/joe-broadhead/aurum/master/scripts/install.sh | bash -s -- --from-release
  AURUM_VERSION=v0.0.4 ./scripts/install.sh --from-release
EOF
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required tool: $1" >&2
    exit 1
  fi
}

detect_asset() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"
  case "${os}-${arch}" in
    linux-x86_64|linux-amd64) echo "aurum-linux-x86_64" ;;
    darwin-arm64|darwin-aarch64) echo "aurum-macos-arm64" ;;
    darwin-x86_64)
      echo "error: prebuilt Intel Mac binaries are not published; use --from-source" >&2
      exit 1
      ;;
    mingw*|msys*|cygwin*|windows*)
      echo "aurum-windows-x86_64.exe"
      ;;
    *)
      echo "error: unsupported platform ${os}/${arch}" >&2
      exit 1
      ;;
  esac
}

install_from_source() {
  need cargo
  need cmake
  if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "warning: ffmpeg not found; non-WAV inputs will fail until it is installed" >&2
  fi
  echo "Installing aurum CLI from source (${ROOT}) …"
  cargo install --path "${ROOT}/crates/aurum" --locked --force --root "${INSTALL_DIR%/bin}" 2>/dev/null \
    || cargo install --path "${ROOT}/crates/aurum" --locked --force
  # cargo install --root puts bin under root/bin
  if [[ -x "${INSTALL_DIR}/aurum" ]]; then
    :
  elif [[ -x "${HOME}/.cargo/bin/aurum" ]]; then
    mkdir -p "${INSTALL_DIR}"
    # Prefer cargo default if INSTALL_DIR not writable path from --root
    :
  fi
  hash -r 2>/dev/null || true
  if command -v aurum >/dev/null 2>&1; then
    echo "Installed: $(command -v aurum)"
    aurum --version
  else
    echo "Installed under cargo bin dir; ensure ~/.cargo/bin or ${INSTALL_DIR} is on PATH" >&2
  fi
}

release_tag() {
  if [[ -n "${VERSION}" ]]; then
    echo "${VERSION}"
    return
  fi
  need curl
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -1
}

install_from_release() {
  need curl
  need shasum || need sha256sum
  local tag asset url sums tmpdir bin
  tag="$(release_tag)"
  if [[ -z "${tag}" ]]; then
    echo "error: could not resolve release tag" >&2
    exit 1
  fi
  asset="$(detect_asset)"
  url="https://github.com/${REPO}/releases/download/${tag}"
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "${tmpdir}"' EXIT

  echo "Downloading ${asset} (${tag}) …"
  curl -fsSL -o "${tmpdir}/${asset}" "${url}/${asset}"
  curl -fsSL -o "${tmpdir}/SHA256SUMS" "${url}/SHA256SUMS"

  echo "Verifying checksum …"
  (
    cd "${tmpdir}"
    if command -v shasum >/dev/null 2>&1; then
      grep " ${asset}\$" SHA256SUMS | shasum -a 256 -c -
    else
      grep " ${asset}\$" SHA256SUMS | sha256sum -c -
    fi
  )

  mkdir -p "${INSTALL_DIR}"
  bin="${INSTALL_DIR}/aurum"
  if [[ "${asset}" == *.exe ]]; then
    bin="${INSTALL_DIR}/aurum.exe"
  fi
  install -m 755 "${tmpdir}/${asset}" "${bin}"
  echo "Installed: ${bin}"
  "${bin}" --version || true
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      echo "Tip: add ${INSTALL_DIR} to PATH" >&2
      ;;
  esac
}

uninstall_binary() {
  local bin="${INSTALL_DIR}/aurum"
  [[ -f "${INSTALL_DIR}/aurum.exe" ]] && bin="${INSTALL_DIR}/aurum.exe"
  if [[ -f "${bin}" ]]; then
    rm -f "${bin}"
    echo "Removed ${bin}"
  else
    echo "No binary at ${bin}"
  fi
  echo "Cache and config were preserved (typically ~/.cache/aurum and platform config dirs)."
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --from-source) MODE=source; shift ;;
    --from-release|--upgrade) MODE=release; shift ;;
    --uninstall) MODE=uninstall; shift ;;
    --version) VERSION="$2"; shift 2 ;;
    --install-dir) INSTALL_DIR="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage; exit 1 ;;
  esac
done

if [[ "${MODE}" == "auto" ]]; then
  if [[ -f "${ROOT}/crates/aurum/Cargo.toml" ]]; then
    MODE=source
  else
    MODE=release
  fi
fi

case "${MODE}" in
  source) install_from_source ;;
  release) install_from_release ;;
  uninstall) uninstall_binary ;;
  *) echo "internal error: mode=${MODE}" >&2; exit 1 ;;
esac
