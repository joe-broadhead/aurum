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

commit="${GITHUB_SHA:-$(git -C "${ROOT}" rev-parse HEAD 2>/dev/null || true)}"
if ! [[ "${commit}" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "source_commit must be a full 40-char hex SHA (got '${commit:-empty}')" >&2
  exit 1
fi
date_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
workflow="${GITHUB_WORKFLOW:-local}"
run_id="${GITHUB_RUN_ID:-0}"
repo="${GITHUB_REPOSITORY:-joe-broadhead/aurum}"
server="${GITHUB_SERVER_URL:-https://github.com}"
# Optional pinned toolchain evidence for the stronger 1.0 claim (JOE-1919).
rustc_version="$(rustc --version 2>/dev/null || echo unknown)"
cargo_version="$(cargo --version 2>/dev/null || echo unknown)"

python3 - <<'PY' "${TAG}" "${commit}" "${date_utc}" "${workflow}" "${run_id}" "${repo}" "${server}" "${rustc_version}" "${cargo_version}"
import hashlib, json, re, sys
from pathlib import Path

tag, commit, date_utc, workflow, run_id, repo, server, rustc_version, cargo_version = sys.argv[1:10]
if not re.fullmatch(r"[0-9a-fA-F]{40}", commit):
    print(f"invalid source_commit: {commit!r}", file=sys.stderr)
    sys.exit(1)
skip = {"PROVENANCE.json", "PROVENANCE.txt", "SHA256SUMS", "SHA256SUMS.sig", "SHA256SUMS.bundle"}
assets = []
for p in sorted(Path(".").iterdir()):
    if not p.is_file() or p.name in skip:
        continue
    data = p.read_bytes()
    kind = "other"
    name = p.name
    if name.startswith("aurum-") and not name.endswith((".json", ".md", ".txt")):
        kind = "cli-binary"
    elif name.endswith((".cdx.json", ".spdx.json")) or "sbom" in name.lower():
        kind = "sbom"
    elif name in ("native-components.md",) or name.startswith("aurum-sbom"):
        kind = "native-inventory"
    assets.append({
        "name": p.name,
        "kind": kind,
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
        "rustc_version": rustc_version,
        "cargo_version": cargo_version,
    },
    "assets": assets,
    "notes": [
        "Checksums in SHA256SUMS are authoritative for byte equality.",
        "Release workflow attaches required cosign keyless bundle as SHA256SUMS.bundle (JOE-1882).",
        "source_commit is a full 40-char git object id; verifiers reject prefix matches (JOE-1919).",
        "Asset kind tags relate binaries/SBOMs/inventories for per-artifact evidence indexing.",
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
