#!/usr/bin/env bash
# Fail-closed provider evidence freshness gate (JOE-2223).
#
# Every `supported` claim in evals/provider-evidence/index.json must have a
# fresh passing evidence record (local is exempt from network smoke).
# Missing/stale remote supported evidence fails the release gate.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

INDEX="evals/provider-evidence/index.json"
if [[ ! -f "$INDEX" ]]; then
  echo "missing evidence index: $INDEX" >&2
  exit 1
fi

# Policy unit tests (always required).
cargo test -p aurum-core --lib --locked evidence -- --quiet

python3 - <<'PY'
import json, sys, time
from pathlib import Path

idx = json.loads(Path("evals/provider-evidence/index.json").read_text())
assert idx.get("schema_version") == 1, idx.get("schema_version")
now = int(time.time())
MAX_AGE = 30 * 24 * 3600

records = []
for p in sorted(Path("evals/provider-evidence").glob("*.json")):
    if p.name == "index.json":
        continue
    records.append(json.loads(p.read_text()))

def route_key(provider, op, model, voice=None):
    return f"{provider}:{op}:{model}:{voice or '-'}"

by_route = {}
for r in records:
    k = route_key(r["provider_id"], r["operation"], r["model_id"], r.get("voice_alias"))
    by_route.setdefault(k, []).append(r)

findings = []
for claim in idx.get("supported_claims") or []:
    if not claim.get("required_for_release", True):
        continue
    provider = claim["provider_id"]
    key = route_key(
        provider, claim["operation"], claim["model_id"], claim.get("voice_alias")
    )
    if provider == "local":
        findings.append(("pass", key, "local_supported"))
        continue
    best = [
        r
        for r in (by_route.get(key) or [])
        if r.get("passed") and r.get("auth_ok")
    ]
    if not best:
        findings.append(("fail", key, "missing_or_failing_evidence"))
        continue
    best.sort(key=lambda r: r.get("executed_at_unix", 0), reverse=True)
    r = best[0]
    age = now - int(r.get("executed_at_unix", 0))
    exp = r.get("expires_at_unix")
    stale = (exp is not None and now > int(exp)) or age > MAX_AGE
    if stale:
        findings.append(("fail", key, "stale_evidence"))
    elif r.get("support_tier") != "supported":
        findings.append(("fail", key, "tier_mismatch"))
    else:
        findings.append(("pass", key, "ok"))

for sev, route, code in findings:
    print(f"{sev.upper()} {code}: {route}")
failed = [f for f in findings if f[0] == "fail"]
if failed:
    print(
        "provider evidence gate FAILED — demote route, restore evidence, or remove supported claim",
        file=sys.stderr,
    )
    sys.exit(1)
print(f"OK provider evidence gate ({len(findings)} claim(s))")
PY
