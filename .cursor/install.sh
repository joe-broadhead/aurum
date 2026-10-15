#!/usr/bin/env bash
# Idempotent Cloud Agent bootstrap for aurum.
# Pins match CI (.github/workflows/ci.yml) and workspace MSRV (1.89.0).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

log() { printf '+ %s\n' "$*"; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1
}

ensure_apt_packages() {
  # libstdc++-14-dev: Cursor default images often point /usr/bin/c++ at clang,
  # which selects the GCC 14 installation; without 14's libstdc++.a, whisper-rs
  # cmake configure fails with "cannot find -lstdc++".
  local pkgs=(
    build-essential
    g++
    libstdc++-14-dev
    cmake
    pkg-config
    libasound2-dev
    ffmpeg
    python3
    python3-pip
    python3-venv
    ca-certificates
    curl
    git
  )
  local missing=()
  local p
  for p in "${pkgs[@]}"; do
    if ! dpkg -s "$p" >/dev/null 2>&1; then
      missing+=("$p")
    fi
  done
  if ((${#missing[@]} == 0)); then
    log "apt packages already present"
    return 0
  fi
  if ! need_cmd sudo; then
    echo "missing packages (${missing[*]}) and sudo is unavailable" >&2
    exit 1
  fi
  log "installing apt packages: ${missing[*]}"
  sudo apt-get update
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "${missing[@]}"
}

ensure_rust() {
  # Prefer existing rustup (Dockerfile or default image). Install if missing.
  if ! need_cmd rustup; then
    log "installing rustup (stable + rustfmt/clippy)"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal --component rustfmt,clippy
    if [[ -f "${HOME}/.cargo/env" ]]; then
      # shellcheck disable=SC1091
      source "${HOME}/.cargo/env"
    fi
    # Login shells may not load ~/.cargo/env; keep tools on PATH.
    if [[ -d "${HOME}/.cargo/bin" ]] && need_cmd sudo; then
      for bin in cargo rustc rustup rustfmt cargo-clippy clippy-driver; do
        if [[ -x "${HOME}/.cargo/bin/${bin}" ]]; then
          sudo ln -sfn "${HOME}/.cargo/bin/${bin}" "/usr/local/bin/${bin}"
        fi
      done
    fi
  fi

  export PATH="${CARGO_HOME:-${HOME}/.cargo}/bin:/usr/local/cargo/bin:${PATH}"

  log "ensuring stable toolchain + rustfmt/clippy (CI default)"
  rustup toolchain install stable --profile minimal --component rustfmt,clippy
  rustup component add rustfmt clippy --toolchain stable
  rustup default stable

  # MSRV gate in CI uses exactly 1.89.0
  log "ensuring MSRV toolchain 1.89.0"
  rustup toolchain install 1.89.0 --profile minimal

  rustc --version
  cargo --version
}

ensure_cargo_registry() {
  log "cargo fetch --locked"
  cargo fetch --locked
}

ensure_docs_venv() {
  log "docs Python venv (.venv + docs/requirements.txt)"
  if [[ ! -d .venv ]]; then
    python3 -m venv .venv
  fi
  .venv/bin/pip install -q --upgrade pip
  .venv/bin/pip install -q -r docs/requirements.txt
}

# Optional CI security tools (pinned in security job). Skip if already present.
ensure_security_tools() {
  export PATH="${CARGO_HOME:-${HOME}/.cargo}/bin:/usr/local/cargo/bin:${PATH}"
  local audit_ver="0.22.2"
  local deny_ver="0.19.0"
  if ! need_cmd cargo-audit || ! cargo audit --version 2>/dev/null | grep -q "${audit_ver}"; then
    log "cargo install cargo-audit --locked --version ${audit_ver}"
    cargo install cargo-audit --locked --version "${audit_ver}" --force
  else
    log "cargo-audit ${audit_ver} already present"
  fi
  if ! need_cmd cargo-deny || ! cargo deny --version 2>/dev/null | grep -q "${deny_ver}"; then
    log "cargo install cargo-deny --locked --version ${deny_ver}"
    cargo install cargo-deny --locked --version "${deny_ver}" --force
  else
    log "cargo-deny ${deny_ver} already present"
  fi
}

main() {
  ensure_apt_packages
  ensure_rust
  ensure_cargo_registry
  ensure_docs_venv
  # Security tools are used by the CI security job; keep them available for agents.
  ensure_security_tools
  log "install complete"
}

main "$@"
