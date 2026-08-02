#!/usr/bin/env bash
# Prepare / validate the STT quality observatory corpus (JOE-2216).
#
# - Default: validate the redistributable core only (safe for CI / clean checkout).
# - --production: require a previously fetched production pack under
#   evals/observatory/cache/ and enforce coverage minima.
# - --slot NAME: document the fetch recipe for a production asset slot.
#
# Never requires private Plaud material. Never uploads restrictive-license audio
# as a public CI artifact.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CORE="${ROOT}/evals/observatory/corpus.core.json"
PROD_MANIFEST="${ROOT}/evals/observatory/corpus.production.manifest.json"
CACHE_DIR="${ROOT}/evals/observatory/cache"
MODE="core"
SLOT=""

usage() {
  cat <<EOF
Usage: $(basename "$0") [--core|--production|--slot NAME] [--cache-dir DIR]

  --core         Validate redistributable core corpus (default)
  --production   Validate production pack presence + coverage (operator machine)
  --slot NAME    Print fetch recipe for a production asset slot
  --cache-dir D  Override cache directory (default: evals/observatory/cache)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --core) MODE="core"; shift ;;
    --production) MODE="production"; shift ;;
    --slot) SLOT="$2"; MODE="slot"; shift 2 ;;
    --cache-dir) CACHE_DIR="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown: $1" >&2; usage; exit 2 ;;
  esac
done

need_file() {
  local f="$1"
  if [[ ! -f "$f" ]]; then
    echo "missing required file: $f" >&2
    exit 1
  fi
}

validate_core_json() {
  need_file "$CORE"
  CORE_PATH="$CORE" python3 - <<'PY'
import json, os, sys
from pathlib import Path
p = Path(os.environ["CORE_PATH"])
data = json.loads(p.read_text())
assert data.get("schema_version") == 1, data.get("schema_version")
assert data.get("fixtures"), "no fixtures"
ids = [f["id"] for f in data["fixtures"]]
assert len(ids) == len(set(ids)), "duplicate fixture ids"
for f in data["fixtures"]:
    assert f.get("license"), f"fixture {f.get('id')} missing license"
    assert "reference" in f, f"fixture {f.get('id')} missing reference"
print(f"OK core corpus: {len(ids)} fixtures, version={data.get('corpus_version')}")
PY
}

print_slot_recipe() {
  need_file "$PROD_MANIFEST"
  SLOT_NAME="$SLOT" PROD_PATH="$PROD_MANIFEST" CACHE_PATH="$CACHE_DIR" python3 - <<'PY'
import json, os, sys
from pathlib import Path
slot = os.environ["SLOT_NAME"]
data = json.loads(Path(os.environ["PROD_PATH"]).read_text())
slots = {s["slot_id"]: s for s in data.get("asset_slots", [])}
if slot not in slots:
    print(f"unknown slot {slot!r}; known: {sorted(slots)}", file=sys.stderr)
    sys.exit(1)
s = slots[slot]
print(f"slot: {s['slot_id']}")
print(f"role: {s.get('role','')}")
print(f"license: {s.get('license_family','')}")
print(f"fetch: {s.get('fetch','')}")
print(f"redistributable_in_repo: {s.get('redistributable_in_repo')}")
if s.get("use_restrictions"):
    print(f"restrictions: {s['use_restrictions']}")
print(f"min_duration_secs: {s.get('min_duration_secs')}")
if s.get("notes"):
    print(f"notes: {s['notes']}")
print()
print("Operator steps:")
print("  1. Create cache dir:", os.environ["CACHE_PATH"])
print("  2. Download licensed sources for this slot only")
print("  3. Write SHA-256 lockfile under cache/<slot>/SHA256SUMS")
print("  4. Generate fixture entries into cache/corpus.production.json")
print("  5. Re-run with --production to enforce coverage minima")
print("Never commit private user audio or Plaud exports.")
PY
}

check_production() {
  need_file "$PROD_MANIFEST"
  local pack="${CACHE_DIR}/corpus.production.json"
  if [[ ! -f "$pack" ]]; then
    echo "production pack not found at ${pack}" >&2
    echo "Fetch asset slots documented in ${PROD_MANIFEST}, then generate the pack." >&2
    echo "CI does not require this pack — use --core." >&2
    exit 1
  fi
  PACK_PATH="$pack" PROD_PATH="$PROD_MANIFEST" python3 - <<'PY'
import json, os, sys
from pathlib import Path
pack = Path(os.environ["PACK_PATH"])
manifest = json.loads(Path(os.environ["PROD_PATH"]).read_text())
data = json.loads(pack.read_text())
targets = manifest["coverage_targets"]
fixtures = data.get("fixtures") or []
duration = sum(float(f.get("duration_secs") or 0) for f in fixtures)
speakers = {f.get("speaker_id") for f in fixtures if f.get("speaker_id")}
accents = set()
for f in fixtures:
    for t in f.get("tags") or []:
        if str(t).startswith("accent_"):
            accents.add(t)
long_form = sum(
    1 for f in fixtures
    if "long_form" in (f.get("tags") or []) or float(f.get("duration_secs") or 0) > 600
)
errors = []
if duration < float(targets["min_duration_secs"]):
    errors.append(f"duration {duration:.1f}s < {targets['min_duration_secs']}")
if len(speakers) < int(targets["min_speakers"]):
    errors.append(f"speakers {len(speakers)} < {targets['min_speakers']}")
if len(accents) < int(targets["min_english_accents"]):
    errors.append(f"accents {len(accents)} < {targets['min_english_accents']}")
if long_form < int(targets["min_long_form_over_10min"]):
    errors.append(f"long_form {long_form} < {targets['min_long_form_over_10min']}")
if errors:
    print("PRODUCTION COVERAGE FAIL:", "; ".join(errors), file=sys.stderr)
    sys.exit(1)
print(
    f"OK production pack: {len(fixtures)} fixtures, {duration:.1f}s, "
    f"{len(speakers)} speakers, {len(accents)} accents, {long_form} long-form"
)
PY
}

case "$MODE" in
  core) validate_core_json ;;
  production) check_production ;;
  slot) print_slot_recipe ;;
  *) echo "bad mode" >&2; exit 2 ;;
esac
