#!/usr/bin/env bash
# Generate structured PROVENANCE.json for a release asset directory (JOE-1860).
#
# Usage (from repo root):
#   ./scripts/generate_provenance.sh <release-assets-dir> <release-tag>
#
# Environment (optional, filled from GitHub Actions when present):
#   GITHUB_WORKFLOW, GITHUB_RUN_ID, GITHUB_SHA, GITHUB_REPOSITORY, GITHUB_SERVER_URL
set -euo pipefail

DIR="${1:-}"
TAG="${2:-}"
if [ -z "${DIR}" ] || [ -z "${TAG}" ] || [ ! -d "${DIR}" ]; then
  echo "usage: $0 <release-assets-dir> <release-tag>" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${DIR}"

commit="${GITHUB_SHA:-$(git -C "${ROOT}" rev-parse HEAD 2>/dev/null || echo unknown)}"
date_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
workflow="${GITHUB_WORKFLOW:-local}"
run_id="${GITHUB_RUN_ID:-0}"
repo="${GITHUB_REPOSITORY:-joe-broadhead/aurum}"
server="${GITHUB_SERVER_URL:-https://github.com}"

python3 - <<'PY' "${TAG}" "${commit}" "${date_utc}" "${workflow}" "${run_id}" "${repo}" "${server}"
import hashlib, json, sys
from pathlib import Path

tag, commit, date_utc, workflow, run_id, repo, server = sys.argv[1:8]
skip = {"PROVENANCE.json", "PROVENANCE.txt", "SHA256SUMS", "SHA256SUMS.sig", "SHA256SUMS.bundle"}
assets = []
for p in sorted(Path(".").iterdir()):
    if not p.is_file() or p.name in skip:
        continue
    data = p.read_bytes()
    assets.append({
        "name": p.name,
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    })

doc = {
    "schema_version": 1,
    "release_tag": tag,
    "source_commit": commit,
    "generated_at_utc": date_utc,
    "workflow": workflow,
    "run_id": str(run_id),
    "repository": repo,
    "source_url": f"{server}/{repo}/tree/{commit}",
    "release_url": f"{server}/{repo}/releases/tag/{tag}",
    "builder": {
        "kind": "github-actions" if run_id not in ("0", "", "local") else "local",
        "workflow": workflow,
        "run_id": str(run_id),
    },
    "assets": assets,
    "notes": [
        "Checksums in SHA256SUMS are authoritative for byte equality.",
        "Release workflow attaches required cosign keyless bundle as SHA256SUMS.bundle (JOE-1882).",
        "See docs/operations/provenance.md for verification (AURUM_REQUIRE_COSIGN=1).",
    ],
}
Path("PROVENANCE.json").write_text(json.dumps(doc, indent=2) + "\n")
# Keep legacy PROVENANCE.txt for human eyeballing
Path("PROVENANCE.txt").write_text(
    "\n".join([
        f"release_tag={tag}",
        f"source_commit={commit}",
        f"generated_at_utc={date_utc}",
        f"workflow={workflow}",
        f"run_id={run_id}",
        f"repository={repo}",
        f"asset_count={len(assets)}",
        "",
    ])
)
print(f"Wrote PROVENANCE.json ({len(assets)} assets)")
PY
