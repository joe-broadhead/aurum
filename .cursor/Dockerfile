# Cloud Agent base image for aurum.
# Mirrors Linux deps from .github/workflows/ci.yml (ubuntu-24.04 runners).
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    CARGO_TERM_COLOR=always \
    RUST_BACKTRACE=1

# CI lint/test: build-essential cmake pkg-config libasound2-dev
# CI test (Linux): ffmpeg
# Docs job: python3 (+ venv/pip in install)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        xz-utils \
        build-essential \
        g++ \
        libstdc++-14-dev \
        cmake \
        pkg-config \
        libasound2-dev \
        ffmpeg \
        python3 \
        python3-pip \
        python3-venv \
        sudo \
    && rm -rf /var/lib/apt/lists/* \
    && echo 'ubuntu ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/ubuntu \
    && chmod 0440 /etc/sudoers.d/ubuntu

# CI uses dtolnay/rust-toolchain with toolchain: stable + rustfmt/clippy.
# Workspace MSRV (rust-version / msrv job): 1.89.0
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal --component rustfmt,clippy \
    && rustup toolchain install 1.89.0 --profile minimal \
    && rustup component add rustfmt clippy --toolchain stable \
    && chmod -R a+rwX /usr/local/rustup /usr/local/cargo

# ubuntu:24.04 already provides the ubuntu user referenced by environment.json
USER ubuntu
WORKDIR /workspace
