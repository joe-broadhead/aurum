#!/usr/bin/env bash
# JOE-2213 — dump reviewed provider catalogues; optional live default checks.
#
# Offline (CI-safe):
#   ./scripts/probe_provider_catalogues.sh
#   ./scripts/probe_provider_catalogues.sh --out dist/provider-catalogue/PROBE_REPORT.md
#
# Live (keys in env; never echoed):
#   OPENROUTER_API_KEY=… OPENAI_API_KEY=… ./scripts/probe_provider_catalogues.sh --live
#
# Exit:
#   0 — defaults present in static registries (and live defaults pass when --live)
#   1 — default missing or live default failure
#   2 — tool error
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE=(--offline)
OUT_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --live) MODE=(--live); shift ;;
    --offline) MODE=(--offline); shift ;;
    --out)
      OUT_ARGS=(--out "$2")
      shift 2
      ;;
    -h|--help)
      sed -n '2,16p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

exec cargo run -q -p aurum-core --example probe_provider_catalogues -- "${MODE[@]}" "${OUT_ARGS[@]}"
