#!/usr/bin/env bash
# Verify a release asset directory (JOE-1635 / JOE-1859 / JOE-1860).
#
# Fail-closed checks:
#   1. SHA256SUMS present and every listed file matches
#   2. Formal SBOM artifacts present and parse as JSON with required keys
#   3. PROVENANCE.json present and consistent with git/tag metadata when provided
#
# Usage:
#   ./scripts/verify_release_assets.sh <release-assets-dir>
#   AURUM_EXPECT_TAG=v0.0.11 AURUM_EXPECT_COMMIT=<sha> ./scripts/verify_release_assets.sh dist/release
set -euo pipefail

DIR="${1:-}"
if [ -z "${DIR}" ] || [ ! -d "${DIR}" ]; then
  echo "usage: $0 <release-assets-dir>" >&2
  exit 2
fi

cd "${DIR}"
echo "== verify_release_assets: $(pwd) =="

if [ ! -f SHA256SUMS ]; then
  echo "SHA256SUMS missing" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  sha256sum -c SHA256SUMS
elif command -v shasum >/dev/null 2>&1; then
  shasum -a 256 -c SHA256SUMS
else
  echo "no sha256 tool available" >&2
  exit 1
fi
echo "Checksums OK."

# --- Formal SBOM (JOE-1859) ---
require_file() {
  if [ ! -f "$1" ]; then
    echo "required release artifact missing: $1" >&2
    exit 1
  fi
}

require_file aurum.cdx.json
require_file aurum.spdx.json
require_file aurum-sbom-inventory.json

python3 - <<'PY'
import json, sys
from pathlib import Path

def load(name):
    p = Path(name)
    try:
        return json.loads(p.read_text())
    except Exception as e:
        print(f"invalid JSON: {name}: {e}", file=sys.stderr)
        sys.exit(1)

cdx = load("aurum.cdx.json")
if cdx.get("bomFormat") != "CycloneDX":
    print("aurum.cdx.json: bomFormat must be CycloneDX", file=sys.stderr)
    sys.exit(1)
if not str(cdx.get("specVersion", "")).startswith("1."):
    print("aurum.cdx.json: unsupported specVersion", file=sys.stderr)
    sys.exit(1)
if not cdx.get("components"):
    print("aurum.cdx.json: components empty", file=sys.stderr)
    sys.exit(1)

spdx = load("aurum.spdx.json")
if not str(spdx.get("spdxVersion", "")).startswith("SPDX-2."):
    print("aurum.spdx.json: spdxVersion must be SPDX-2.x", file=sys.stderr)
    sys.exit(1)
if not spdx.get("packages"):
    print("aurum.spdx.json: packages empty", file=sys.stderr)
    sys.exit(1)

inv = load("aurum-sbom-inventory.json")
if inv.get("schema_version") != 1:
    print("aurum-sbom-inventory.json: unexpected schema_version", file=sys.stderr)
    sys.exit(1)
if not inv.get("packages"):
    print("aurum-sbom-inventory.json: packages empty", file=sys.stderr)
    sys.exit(1)

print(f"SBOM OK (CycloneDX components={len(cdx['components'])}, SPDX packages={len(spdx['packages'])})")
PY

# --- Provenance (JOE-1860) ---
if [ -f PROVENANCE.json ]; then
  python3 - <<'PY'
import json, os, sys
from pathlib import Path

doc = json.loads(Path("PROVENANCE.json").read_text())
for key in ("schema_version", "release_tag", "source_commit", "generated_at_utc"):
    if key not in doc:
        print(f"PROVENANCE.json missing key: {key}", file=sys.stderr)
        sys.exit(1)
if doc.get("schema_version") != 1:
    print("PROVENANCE.json: unsupported schema_version", file=sys.stderr)
    sys.exit(1)
expect_tag = os.environ.get("AURUM_EXPECT_TAG", "").strip()
expect_commit = os.environ.get("AURUM_EXPECT_COMMIT", "").strip()
if expect_tag and doc["release_tag"] != expect_tag:
    print(f"PROVENANCE tag mismatch: {doc['release_tag']} != {expect_tag}", file=sys.stderr)
    sys.exit(1)
if expect_commit and not (
    doc["source_commit"] == expect_commit
    or doc["source_commit"].startswith(expect_commit)
    or expect_commit.startswith(doc["source_commit"][:12])
):
    print(f"PROVENANCE commit mismatch: {doc['source_commit']} vs {expect_commit}", file=sys.stderr)
    sys.exit(1)
# Assets listed in provenance must exist and match digests when provided
assets = doc.get("assets") or []
for a in assets:
    name = a.get("name")
    digest = a.get("sha256")
    if not name or not digest:
        print("PROVENANCE assets entries need name+sha256", file=sys.stderr)
        sys.exit(1)
    p = Path(name)
    if not p.is_file():
        print(f"PROVENANCE asset missing on disk: {name}", file=sys.stderr)
        sys.exit(1)
    import hashlib
    got = hashlib.sha256(p.read_bytes()).hexdigest()
    if got != digest:
        print(f"PROVENANCE digest mismatch for {name}", file=sys.stderr)
        sys.exit(1)
print("PROVENANCE.json OK")
PY
elif [ -f PROVENANCE.txt ]; then
  echo "PROVENANCE.txt present (legacy text form); prefer PROVENANCE.json for structured verify."
else
  echo "WARNING: no PROVENANCE.json or PROVENANCE.txt (allowed for local dry-runs only)" >&2
fi

# --- Cosign keyless (JOE-1882) ---
# Official releases set AURUM_REQUIRE_COSIGN=1 and publish SHA256SUMS.bundle.
# Local dry-runs may omit the bundle unless AURUM_REQUIRE_COSIGN=1.
require_cosign="${AURUM_REQUIRE_COSIGN:-0}"
has_bundle=0
if [ -f SHA256SUMS.bundle ] || [ -f SHA256SUMS.sig ]; then
  has_bundle=1
fi

if [ "${require_cosign}" = "1" ] && [ "${has_bundle}" != "1" ]; then
  echo "AURUM_REQUIRE_COSIGN=1 but SHA256SUMS.bundle is missing" >&2
  exit 1
fi

if [ "${has_bundle}" = "1" ] || [ "${require_cosign}" = "1" ]; then
  if ! command -v cosign >/dev/null 2>&1; then
    if [ "${require_cosign}" = "1" ]; then
      echo "cosign required (AURUM_REQUIRE_COSIGN=1) but not installed" >&2
      exit 1
    fi
    echo "Signature bundle present but cosign not installed; skip cryptographic verify."
  else
    issuer="${AURUM_COSIGN_CERTIFICATE_OIDC_ISSUER:-https://token.actions.githubusercontent.com}"
    identity="${AURUM_COSIGN_CERTIFICATE_IDENTITY:-}"
    identity_re="${AURUM_COSIGN_CERTIFICATE_IDENTITY_REGEXP:-}"
    if [ -z "${identity}" ] && [ -z "${identity_re}" ]; then
      if [ "${require_cosign}" = "1" ]; then
        echo "AURUM_REQUIRE_COSIGN=1 requires AURUM_COSIGN_CERTIFICATE_IDENTITY or _REGEXP" >&2
        exit 1
      fi
      echo "SHA256SUMS signature present but AURUM_COSIGN_* identity env not set; skip cryptographic verify."
    else
      echo "Verifying cosign keyless bundle..."
      args=(verify-blob --bundle SHA256SUMS.bundle --certificate-oidc-issuer "${issuer}")
      if [ -n "${identity_re}" ]; then
        args+=(--certificate-identity-regexp "${identity_re}")
      else
        args+=(--certificate-identity "${identity}")
      fi
      cosign "${args[@]}" SHA256SUMS
      echo "cosign verify-blob OK"
    fi
  fi
fi

echo "All release asset checks OK."
