#!/usr/bin/env bash
# Prepare / validate the STT quality observatory corpus (JOE-2216 / JOE-2231 / JOE-2318).
#
# - Default: validate the redistributable core only (safe for CI / clean checkout).
# - --production: require a previously fetched production pack under
#   evals/observatory/cache/ and enforce coverage minima.
# - --slot NAME: document the fetch recipe for a production asset slot.
# - --fetch-slot NAME: actually download/assemble a slot (operator; uses Python helper).
# - --assemble-production: merge slot fixtures → cache/corpus.production.json
# - --score-subset: score a capped production subset with local aurum (real WER path)
# - --recipe-check: CI-safe integrity of production manifest recipes (no audio).
# - --dry-run-production: write a synthetic pack that meets minima to prove the
#   coverage checker (NOT real speech quality evidence).
#
# Never requires private Plaud material. Never uploads restrictive-license audio
# as a public CI artifact.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CORE="${ROOT}/evals/observatory/corpus.core.json"
PROD_MANIFEST="${ROOT}/evals/observatory/corpus.production.manifest.json"
CACHE_DIR="${ROOT}/evals/observatory/cache"
FETCH_PY="${ROOT}/scripts/eval/fetch_production_slots.py"
MODE="core"
SLOT=""
SCORE_MODEL="tiny-q5_1"
SCORE_PROFILE="apple_silicon_metal"
SCORE_MAX=24
AURUM_BIN="${AURUM_BIN:-aurum}"

usage() {
  cat <<EOF
Usage: $(basename "$0") [MODE] [--cache-dir DIR]

  --core                 Validate redistributable core corpus (default)
  --production           Validate production pack presence + coverage (operator machine)
  --slot NAME            Print fetch recipe for a production asset slot
  --fetch-slot NAME      Download/assemble a real slot (or "all-auto")
  --assemble-production  Merge slot fixtures into cache/corpus.production.json
  --score-subset         Run local STT on a capped production subset (real speech)
  --recipe-check         CI-safe: every asset slot has fetch/license/min_duration
  --dry-run-production   Generate synthetic pack under cache/ (NOT real speech evidence)
  --cache-dir D          Override cache directory (default: evals/observatory/cache)
  --model NAME           Model for --score-subset (default: tiny-q5_1)
  --profile NAME         Hardware profile label for --score-subset
  --max-fixtures N       Cap for --score-subset (default: 24)
  --aurum PATH           aurum binary for --score-subset
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --core) MODE="core"; shift ;;
    --production) MODE="production"; shift ;;
    --slot) SLOT="$2"; MODE="slot"; shift 2 ;;
    --fetch-slot) SLOT="$2"; MODE="fetch-slot"; shift 2 ;;
    --assemble-production) MODE="assemble-production"; shift ;;
    --score-subset) MODE="score-subset"; shift ;;
    --recipe-check) MODE="recipe-check"; shift ;;
    --dry-run-production) MODE="dry-run-production"; shift ;;
    --cache-dir) CACHE_DIR="$2"; shift 2 ;;
    --model) SCORE_MODEL="$2"; shift 2 ;;
    --profile) SCORE_PROFILE="$2"; shift 2 ;;
    --max-fixtures) SCORE_MAX="$2"; shift 2 ;;
    --aurum) AURUM_BIN="$2"; shift 2 ;;
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

recipe_check() {
  need_file "$PROD_MANIFEST"
  PROD_PATH="$PROD_MANIFEST" python3 - <<'PY'
import json, os, sys
from pathlib import Path
data = json.loads(Path(os.environ["PROD_PATH"]).read_text())
assert data.get("schema_version") == 1
targets = data.get("coverage_targets") or {}
for k in ("min_duration_secs", "min_speakers", "min_english_accents", "min_long_form_over_10min", "required_scenarios"):
    assert k in targets, f"coverage_targets missing {k}"
slots = data.get("asset_slots") or []
assert slots, "no asset_slots"
errors = []
ids = []
for s in slots:
    sid = s.get("slot_id")
    if not sid:
        errors.append("slot missing slot_id")
        continue
    ids.append(sid)
    for field in ("role", "license_family", "fetch", "min_duration_secs"):
        if s.get(field) is None or s.get(field) == "":
            errors.append(f"{sid}: missing {field}")
    # Speech corpora must not vendor multi-GB audio in-repo; control tones may.
    if s.get("redistributable_in_repo") is True and "control" not in sid and "silence" not in sid:
        errors.append(f"{sid}: non-control production audio must not be redistributable_in_repo")
    fetch = str(s.get("fetch") or "")
    ok_fetch = (
        "prepare_stt_observatory_corpus.sh" in fetch
        or fetch.startswith("http")
        or fetch.startswith("in-repo")
    )
    if not ok_fetch:
        errors.append(f"{sid}: fetch must reference prepare script, URL, or in-repo path")
if len(ids) != len(set(ids)):
    errors.append("duplicate slot_id values")
if errors:
    print("RECIPE CHECK FAIL:", "; ".join(errors), file=sys.stderr)
    sys.exit(1)
print(f"OK recipe integrity: {len(slots)} slots, coverage_targets present")
PY
}

dry_run_production() {
  # Synthetic fixtures that satisfy minima — explicit dry_run tag; not product WER evidence.
  need_file "$PROD_MANIFEST"
  mkdir -p "$CACHE_DIR"
  local pack="${CACHE_DIR}/corpus.production.json"
  PACK_PATH="$pack" PROD_PATH="$PROD_MANIFEST" python3 - <<'PY'
import json, os
from pathlib import Path
manifest = json.loads(Path(os.environ["PROD_PATH"]).read_text())
targets = manifest["coverage_targets"]
min_dur = float(targets["min_duration_secs"])
min_speakers = int(targets["min_speakers"])
min_accents = int(targets["min_english_accents"])
min_long = int(targets["min_long_form_over_10min"])
scenarios = list(targets.get("required_scenarios") or [])
fixtures = []
# Build enough speakers and accents; pad duration with long-form assemblies.
for i in range(max(min_speakers, 20)):
    accent = f"accent_{['us','uk','in','au','ie','za'][i % 6]}"
    tags = ["dry_run_synthetic", "conversational", accent]
    if i < min_long:
        tags.append("long_form")
        dur = 700.0
    else:
        dur = max(30.0, (min_dur / max(min_speakers, 1)) + 5.0)
    for sc in scenarios:
        if sc not in tags:
            tags.append(sc)
            break
    fixtures.append({
        "id": f"dry-run-speaker-{i:03d}",
        "speaker_id": f"spk-{i:03d}",
        "duration_secs": dur,
        "license": "synthetic CC0 dry-run — not real speech quality evidence",
        "reference": f"synthetic dry-run utterance {i}",
        "tags": tags,
        "audio_path": None,
        "notes": "JOE-2231 dry-run coverage probe only",
    })
# Ensure total duration
total = sum(f["duration_secs"] for f in fixtures)
if total < min_dur:
    fixtures[0]["duration_secs"] += (min_dur - total + 1.0)
    fixtures[0]["tags"] = list(set(fixtures[0]["tags"] + ["long_form"]))
pack = {
    "schema_version": 1,
    "name": "aurum-observatory-production-dry-run",
    "corpus_version": "observatory-production-dry-run-v1",
    "description": "SYNTHETIC dry-run pack for coverage gate only. Not a quality claim.",
    "dry_run": True,
    "fixtures": fixtures,
}
path = Path(os.environ["PACK_PATH"])
path.write_text(json.dumps(pack, indent=2) + "\n")
print(f"wrote synthetic dry-run pack: {path} ({len(fixtures)} fixtures)")
print("WARNING: dry-run pack is not real multi-speaker speech evidence.")
PY
  check_production
}

fetch_slot() {
  need_file "$FETCH_PY"
  if [[ -z "$SLOT" ]]; then
    echo "--fetch-slot requires a slot name or all-auto" >&2
    exit 2
  fi
  mkdir -p "$CACHE_DIR"
  python3 "$FETCH_PY" --cache-dir "$CACHE_DIR" --repo-root "$ROOT" fetch "$SLOT"
}

assemble_production() {
  need_file "$FETCH_PY"
  mkdir -p "$CACHE_DIR"
  python3 "$FETCH_PY" --cache-dir "$CACHE_DIR" --repo-root "$ROOT" assemble
}

score_subset() {
  need_file "$FETCH_PY"
  python3 "$FETCH_PY" --cache-dir "$CACHE_DIR" --repo-root "$ROOT" score-subset \
    --model "$SCORE_MODEL" \
    --profile "$SCORE_PROFILE" \
    --max-fixtures "$SCORE_MAX" \
    --aurum "$AURUM_BIN" \
    --out-dir "${ROOT}/evals/reports/stt"
}

case "$MODE" in
  core) validate_core_json ;;
  production) check_production ;;
  slot) print_slot_recipe ;;
  fetch-slot) fetch_slot ;;
  assemble-production) assemble_production ;;
  score-subset) score_subset ;;
  recipe-check) recipe_check ;;
  dry-run-production) dry_run_production ;;
  *) echo "bad mode" >&2; exit 2 ;;
esac
