#!/usr/bin/env bash
# Operator helper for named-hardware STT smoke timing (JOE-1739).
# Requires a pre-cached model; does not download unpinned artifacts.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="local"
MODEL="tiny-q5_1"
FIXTURE="${ROOT}/tests/fixtures/sample.wav"
OUT_DIR="${ROOT}/evals/reports"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile) PROFILE="$2"; shift 2 ;;
    --model) MODEL="$2"; shift 2 ;;
    --fixture) FIXTURE="$2"; shift 2 ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    *) echo "unknown: $1" >&2; exit 1 ;;
  esac
done
mkdir -p "${OUT_DIR}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${OUT_DIR}/perf-${PROFILE}-${MODEL}-${STAMP}.json"
if [[ ! -f "${FIXTURE}" ]]; then
  echo "missing fixture ${FIXTURE}" >&2
  exit 1
fi
need_aurum() {
  if command -v aurum >/dev/null 2>&1; then
    command -v aurum
  else
    echo "cargo:run"
  fi
}
run_once() {
  local start end
  start=$(python3 - <<'PY'
import time; print(time.time())
PY
)
  if command -v aurum >/dev/null 2>&1; then
    aurum "${FIXTURE}" --model "${MODEL}" --output-file /tmp/aurum-perf-out.txt >/dev/null
  else
    cargo run -q -p aurum-stt -- "${FIXTURE}" --model "${MODEL}" --output-file /tmp/aurum-perf-out.txt >/dev/null
  fi
  end=$(python3 - <<'PY'
import time; print(time.time())
PY
)
  python3 - <<PY
start=float("${start}"); end=float("${end}")
print(f"{(end-start)*1000:.3f}")
PY
}

echo "Running 3 timed transcriptions model=${MODEL} fixture=${FIXTURE}"
samples=()
for i in 1 2 3; do
  ms="$(run_once)"
  samples+=("${ms}")
  echo "  run ${i}: ${ms} ms"
done

python3 - <<PY
import json, statistics, platform, subprocess, os
samples = list(map(float, """${samples[*]}""".split()))
samples.sort()
def pct(p):
    if not samples: return 0.0
    k = int(round((p/100)*(len(samples)-1)))
    return samples[k]
report = {
  "schema_version": 1,
  "kind": "stt_e2e_wall",
  "profile": "${PROFILE}",
  "model": "${MODEL}",
  "fixture": "${FIXTURE}",
  "host": platform.node(),
  "os": platform.platform(),
  "samples_ms": samples,
  "p50_ms": pct(50),
  "p95_ms": pct(95),
  "mean_ms": statistics.mean(samples) if samples else 0,
  "warm": True,
  "notes": "operator script; not a substitute for full concurrency matrix",
}
path = "${OUT}"
with open(path, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2)
print("wrote", path)
PY
