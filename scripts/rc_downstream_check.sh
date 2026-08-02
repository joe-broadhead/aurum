#!/usr/bin/env bash
# Downstream consumer freeze gate (JOE-1903).
#
# 1) Minimal Rust consumer of aurum-core (path dependency, no-default-features)
# 2) C11/C++17 FFI examples against release staticlib (Linux/macOS)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== rc_downstream_check (JOE-1903) =="

TMP="$(mktemp -d "${TMPDIR:-/tmp}/aurum-downstream-XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT

echo "== minimal Rust consumer =="
mkdir -p "${TMP}/consumer/src"
cat > "${TMP}/consumer/Cargo.toml" <<EOF
[package]
name = "aurum-downstream-consumer"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
aurum-core = { path = "${ROOT}/crates/aurum-core", default-features = false }
EOF
cat > "${TMP}/consumer/src/main.rs" <<'EOF'
use aurum_core::domain::{FiniteDurationSecs, ModelId, SampleRateHz};
use aurum_core::dto::STT_RESULT_SCHEMA_VERSION;
use aurum_core::error::ErrorCategory;
use aurum_core::providers::Segment;

fn main() {
    assert_eq!(STT_RESULT_SCHEMA_VERSION, 2);
    let _ = SampleRateHz::try_new(16_000).expect("rate");
    let _ = FiniteDurationSecs::try_new(1.0).expect("dur");
    let _ = ModelId::try_new("tiny-q5_1").expect("id");
    let _ = Segment::try_new(0.0, 0.5, "hi").expect("seg");
    assert_eq!(ErrorCategory::InvalidInput.as_str(), "invalid_input");
    println!("aurum-downstream-consumer OK");
}
EOF
(cd "${TMP}/consumer" && cargo run --locked 2>/dev/null || cargo run)
echo "Rust consumer OK"

echo "== FFI ABI layout =="
cargo test -p aurum-ffi --test abi_layout --no-default-features --locked

echo "== C11/C++17 examples (STT-only staticlib) =="
if [[ "$(uname -s)" == "Windows_NT" ]] || [[ "$(uname -o 2>/dev/null || true)" == Msys* ]]; then
  echo "Windows host link deferred (documented residual); ABI tests still run."
else
  cargo build -p aurum-ffi --release --locked --no-default-features
  INC=crates/aurum-ffi/include
  LIB=target/release
  if [[ "$(uname -s)" == "Darwin" ]]; then
    cc -std=c11 -I "$INC" crates/aurum-ffi/examples/job_cleanup.c \
      "$LIB/libaurum_ffi.a" \
      -lpthread -ldl -lm -lc++ \
      -framework Security -framework CoreFoundation \
      -framework Metal -framework Foundation -framework Accelerate \
      -o "${TMP}/aurum_job_cleanup"
    c++ -std=c++17 -I "$INC" crates/aurum-ffi/examples/engine_raii.cpp \
      "$LIB/libaurum_ffi.a" \
      -lpthread -ldl -lm -lc++ \
      -framework Security -framework CoreFoundation \
      -framework Metal -framework Foundation -framework Accelerate \
      -o "${TMP}/aurum_engine_raii"
  else
    cc -std=c11 -I "$INC" crates/aurum-ffi/examples/job_cleanup.c \
      "$LIB/libaurum_ffi.a" \
      -lpthread -ldl -lm -lstdc++ -o "${TMP}/aurum_job_cleanup"
    c++ -std=c++17 -I "$INC" crates/aurum-ffi/examples/engine_raii.cpp \
      "$LIB/libaurum_ffi.a" \
      -lpthread -ldl -lm -lstdc++ -o "${TMP}/aurum_engine_raii"
  fi
  "${TMP}/aurum_job_cleanup"
  "${TMP}/aurum_engine_raii"
  echo "C/C++ FFI examples OK"
fi

echo "rc_downstream_check.sh OK"
