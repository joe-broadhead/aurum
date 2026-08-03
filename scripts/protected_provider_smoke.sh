#!/usr/bin/env bash
# Protected remote inference smoke + redacted evidence retention (JOE-2229).
#
# Modes:
#   --dry-run   CI-safe: write experimental schema samples, validate, canary-scan.
#               Does NOT claim live vendor inference. Remotes stay experimental.
#   --live      When provider secrets are present, run short synthetic smokes via
#               the Aurum CLI (or skip routes without keys). Writes redacted
#               evidence under OUT. Never auto-promotes to supported in-repo.
#
# Usage:
#   ./scripts/protected_provider_smoke.sh --dry-run [--out dist/provider-smoke]
#   ./scripts/protected_provider_smoke.sh --live [--out dist/provider-smoke]
#
# Privacy: no audio, transcripts, synthesis text, or secrets in retained JSON.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE=""
OUT="${ROOT}/dist/provider-smoke"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) MODE="dry-run"; shift ;;
    --live) MODE="live"; shift ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help)
      sed -n '1,20p' "$0"
      exit 0
      ;;
    *) echo "unknown: $1" >&2; exit 2 ;;
  esac
done

if [[ -z "${MODE}" ]]; then
  echo "usage: $0 --dry-run|--live [--out DIR]" >&2
  exit 2
fi

# Absolute OUT
if [[ "${OUT}" != /* ]]; then
  OUT="${ROOT}/${OUT}"
fi
mkdir -p "${OUT}/records"
VER="$(tr -d '[:space:]' < VERSION)"
COMMIT="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
NOW="$(python3 -c 'import time; print(int(time.time()))')"
EXP="$((NOW + 30 * 24 * 3600))"
RUN_ID="${GITHUB_RUN_ID:-local}"
SUMMARY="${OUT}/SMOKE_SUMMARY.md"

canary_scan() {
  local dir="$1"
  if grep -RInE 'sk-[A-Za-z0-9]{10,}|Bearer [A-Za-z0-9._-]{8,}|BEGIN (RSA |OPENSSH )?PRIVATE|OPENROUTER_API_KEY=|OPENAI_API_KEY=|ELEVENLABS_API_KEY=|XAI_API_KEY=|transcript=|pcm=' \
    "${dir}" 2>/dev/null; then
    echo "FAIL canary scan: secret/payload-like fragment under ${dir}" >&2
    exit 1
  fi
  echo "canary scan OK (${dir})"
}

write_record() {
  # args: file provider op model protocol tier passed auth_ok failure notes latency result_units backend
  # passed/auth_ok: "true" or "false" strings
  local file="$1" provider="$2" op="$3" model="$4" protocol="$5"
  local tier="$6" passed="$7" auth_ok="$8" failure="$9" notes="${10}"
  local latency="${11:-}" result_units="${12:-}" backend="${13:-}"
  FILE="$file" PROVIDER="$provider" OP="$op" MODEL="$model" PROTOCOL="$protocol" \
  TIER="$tier" PASSED="$passed" AUTH_OK="$auth_ok" FAILURE="$failure" NOTES="$notes" \
  LATENCY="$latency" RESULT_UNITS="$result_units" BACKEND="$backend" \
  COMMIT="$COMMIT" VER="$VER" NOW="$NOW" EXP="$EXP" RUN_ID="$RUN_ID" \
  python3 - <<'PY'
import json, os
from pathlib import Path
rec = {
  "schema_version": 1,
  "provider_id": os.environ["PROVIDER"],
  "operation": os.environ["OP"],
  "model_id": os.environ["MODEL"],
  "support_tier": os.environ["TIER"],
  "aurum_commit": os.environ["COMMIT"],
  "aurum_version": os.environ["VER"],
  "protocol_contract": os.environ["PROTOCOL"],
  "executed_at_unix": int(os.environ["NOW"]),
  "workflow_run_id": os.environ["RUN_ID"],
  "auth_ok": os.environ["AUTH_OK"].lower() == "true",
  "passed": os.environ["PASSED"].lower() == "true",
  "failure_category": os.environ["FAILURE"],
  "timestamps_reliable": False,
  "expires_at_unix": int(os.environ["EXP"]),
  "notes": os.environ["NOTES"],
}
if os.environ.get("LATENCY"):
    rec["latency_ms"] = int(os.environ["LATENCY"])
if os.environ.get("RESULT_UNITS"):
    rec["result_units"] = int(os.environ["RESULT_UNITS"])
if os.environ.get("BACKEND"):
    rec["backend_kind"] = os.environ["BACKEND"]
path = Path(os.environ["FILE"])
path.write_text(json.dumps(rec, indent=2) + "\n")
print(f"wrote {path}")
PY
}

validate_records() {
  RECORDS_DIR="${OUT}/records" python3 - <<'PY'
import json, os, sys
from pathlib import Path
root = Path(os.environ["RECORDS_DIR"])
files = sorted(root.glob("*.json"))
assert files, "no evidence records"
for p in files:
    rec = json.loads(p.read_text())
    assert rec.get("schema_version") == 1, p
    assert rec.get("provider_id") and rec.get("model_id"), p
    assert rec.get("executed_at_unix"), p
    notes = rec.get("notes") or ""
    for bad in ("sk-", "Bearer ", "BEGIN_", "transcript=", "pcm="):
        assert bad not in notes, f"{p}: notes contain {bad}"
    for forbidden in ("transcript", "audio", "request_body", "response_body", "api_key"):
        assert forbidden not in rec, f"{p}: forbidden field {forbidden}"
print(f"OK validated {len(files)} evidence record(s)")
PY
}

build_cli() {
  if [[ -x target/release/aurum ]]; then
    echo "${ROOT}/target/release/aurum"
    return
  fi
  cargo build -p aurum-stt --release --locked >/dev/null
  echo "${ROOT}/target/release/aurum"
}

live_route() {
  # name provider op model env_var protocol [extra aurum args...]
  local name="$1" provider="$2" op="$3" model="$4" env_var="$5" protocol="$6"
  shift 6
  local key_val="${!env_var:-}"
  local file="${OUT}/records/${name}.json"
  if [[ -z "${key_val}" ]]; then
    write_record "${file}" "${provider}" "${op}" "${model}" "${protocol}" \
      "experimental" "false" "false" "auth" "skipped_missing_secret" "" "" ""
    echo "SKIP ${name}: ${env_var} unset"
    return 0
  fi
  local cli
  cli="$(build_cli)"
  local start end latency rc=0
  start="$(python3 -c 'import time; print(int(time.time()*1000))')"
  set +e
  if [[ "${op}" == "stt" ]]; then
    # Synthetic tone fixture only — transcript discarded (not retained as evidence).
    "${cli}" transcribe \
      --provider "${provider}" \
      --model "${model}" \
      "$@" \
      evals/audio/tone_440_1s.wav \
      -o txt \
      --output-file "${OUT}/_scratch/${name}.txt" \
      >/dev/null 2>"${OUT}/_scratch/${name}.err"
    rc=$?
  else
    # Short harmless TTS text; WAV discarded after smoke.
    "${cli}" tts \
      --provider "${provider}" \
      --model "${model}" \
      "$@" \
      "Aurum protected smoke." \
      -O "${OUT}/_scratch/${name}.wav" \
      >/dev/null 2>"${OUT}/_scratch/${name}.err"
    rc=$?
  fi
  set -e
  end="$(python3 -c 'import time; print(int(time.time()*1000))')"
  latency=$((end - start))
  # Scrub scratch: drop transcripts/audio; keep size-bounded redacted err only.
  rm -f "${OUT}/_scratch/${name}.txt" "${OUT}/_scratch/${name}.wav" 2>/dev/null || true
  if [[ -f "${OUT}/_scratch/${name}.err" ]]; then
    python3 - <<PY
from pathlib import Path
p = Path("${OUT}/_scratch/${name}.err")
raw = p.read_text(errors="replace")[:2000]
lower = raw.lower()
if any(x in lower for x in ("sk-", "bearer ", "api_key", "authorization:")):
    raw = "redacted_error"
p.write_text(raw)
PY
  fi
  if [[ "${rc}" -eq 0 ]]; then
    write_record "${file}" "${provider}" "${op}" "${model}" "${protocol}" \
      "experimental" "true" "true" "none" "live_smoke_ok_not_auto_promoted" \
      "${latency}" "1" "remote"
    echo "PASS ${name} (${latency}ms)"
  else
    write_record "${file}" "${provider}" "${op}" "${model}" "${protocol}" \
      "experimental" "false" "true" "other" "live_smoke_failed_redacted_err" \
      "${latency}" "" "remote"
    echo "FAIL ${name} rc=${rc} (evidence retained experimental)"
  fi
}

{
  echo "# Provider protected smoke summary (JOE-2229)"
  echo
  echo "- **mode:** ${MODE}"
  echo "- **generated_at_unix:** ${NOW}"
  echo "- **aurum_version:** ${VER}"
  echo "- **source_commit:** ${COMMIT}"
  echo "- **workflow_run_id:** ${RUN_ID}"
  echo "- **out:** ${OUT}"
  echo
  echo "Remotes remain **experimental** until a human promotes reviewed evidence"
  echo "into \`evals/provider-evidence/\` and updates product surfaces."
  echo
} > "${SUMMARY}"

mkdir -p "${OUT}/_scratch"

if [[ "${MODE}" == "dry-run" ]]; then
  # Schema samples only — passed=false, experimental, no network.
  write_record "${OUT}/records/dry-openrouter-stt.json" "openrouter" "stt" \
    "google/gemini-2.5-flash" "openrouter_stt_v1" "experimental" "false" "false" \
    "other" "dry_run_schema_only_no_network" "" "" ""
  write_record "${OUT}/records/dry-openrouter-tts.json" "openrouter" "tts" \
    "hexgrad/kokoro-82m" "openrouter_tts_v1" "experimental" "false" "false" \
    "other" "dry_run_schema_only_no_network" "" "" ""
  write_record "${OUT}/records/dry-openai-stt.json" "openai" "stt" \
    "whisper-1" "openai_stt_v1" "experimental" "false" "false" \
    "other" "dry_run_schema_only_no_network" "" "" ""
  write_record "${OUT}/records/dry-openai-tts.json" "openai" "tts" \
    "tts-1" "openai_tts_v1" "experimental" "false" "false" \
    "other" "dry_run_schema_only_no_network" "" "" ""
  write_record "${OUT}/records/dry-elevenlabs-tts.json" "elevenlabs" "tts" \
    "eleven_multilingual_v2" "elevenlabs_tts_v1" "experimental" "false" "false" \
    "other" "dry_run_schema_only_no_network" "" "" ""
  write_record "${OUT}/records/dry-xai-stt.json" "xai" "stt" \
    "grok-stt" "xai_stt_v1" "experimental" "false" "false" \
    "other" "dry_run_schema_only_no_network" "" "" ""
  write_record "${OUT}/records/dry-xai-tts.json" "xai" "tts" \
    "grok-tts" "xai_tts_v1" "experimental" "false" "false" \
    "other" "dry_run_schema_only_no_network" "" "" ""
  echo "- **routes:** dry-run schema samples only (no live inference)" >> "${SUMMARY}"
elif [[ "${MODE}" == "live" ]]; then
  live_route "openrouter-stt" "openrouter" "stt" "google/gemini-2.5-flash" \
    "OPENROUTER_API_KEY" "openrouter_stt_v1"
  live_route "openrouter-tts" "openrouter" "tts" "hexgrad/kokoro-82m" \
    "OPENROUTER_API_KEY" "openrouter_tts_v1"
  live_route "openai-stt" "openai" "stt" "whisper-1" \
    "OPENAI_API_KEY" "openai_stt_v1"
  live_route "openai-tts" "openai" "tts" "tts-1" \
    "OPENAI_API_KEY" "openai_tts_v1"
  live_route "elevenlabs-tts" "elevenlabs" "tts" "eleven_multilingual_v2" \
    "ELEVENLABS_API_KEY" "elevenlabs_tts_v1" --voice "${ELEVENLABS_VOICE_ALIAS:-Rachel}"
  live_route "xai-stt" "xai" "stt" "grok-stt" \
    "XAI_API_KEY" "xai_stt_v1"
  live_route "xai-tts" "xai" "tts" "grok-tts" \
    "XAI_API_KEY" "xai_tts_v1"
  echo "- **routes:** live attempt per available secret (see records/)" >> "${SUMMARY}"
fi

validate_records
canary_scan "${OUT}/records"
canary_scan "${SUMMARY}"

# Ensure in-repo release evidence pack still only requires local supported routes.
if [[ "${AURUM_SMOKE_SKIP_RELEASE_GATE:-}" != "1" ]]; then
  ./scripts/check_provider_evidence.sh >/dev/null
fi

{
  echo
  echo "## Next steps (human)"
  echo
  echo "1. Review \`${OUT}/records/*.json\` (redacted only)."
  echo "2. For routes that passed live, open a PR that copies reviewed files into"
  echo "   \`evals/provider-evidence/\`, updates \`index.json\` supported claims,"
  echo "   and regenerates product surfaces / changelog."
  echo "3. Do **not** promote from this workflow automatically."
  echo
} >> "${SUMMARY}"

echo "Wrote ${SUMMARY}"
cat "${SUMMARY}"
echo "protected_provider_smoke.sh OK (${MODE})"
