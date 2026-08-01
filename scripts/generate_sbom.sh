#!/usr/bin/env bash
# Generate release SBOM artifacts (JOE-1635 / JOE-1859).
#
# Always produces:
#   - cargo-metadata.json          full locked graph
#   - aurum-sbom-inventory.json    human-oriented inventory (schema_version 1)
#   - aurum-sbom-inventory.md      markdown table
#   - aurum.cdx.json               CycloneDX 1.5 (generated from metadata; no extra tools)
#   - aurum.spdx.json              SPDX 2.3 lite document
#   - native-components.md         native/runtime notes
#
# Optional: if cargo-cyclonedx is installed, also writes aurum.cdx.tool.json
# for comparison (not required).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${1:-dist/sbom}"
mkdir -p "${OUT_DIR}"

version="$(tr -d '[:space:]' < VERSION)"
commit="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
date_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo "Generating SBOM inventory for v${version} @ ${commit}"

cargo metadata --format-version 1 --locked > "${OUT_DIR}/cargo-metadata.json"

python3 - <<'PY' "${OUT_DIR}" "${version}" "${commit}" "${date_utc}"
import json, sys, hashlib, uuid
from pathlib import Path

out, version, commit, date_utc = Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
meta = json.loads((out / "cargo-metadata.json").read_text())
pkgs = []
for p in meta.get("packages", []):
    pkgs.append({
        "name": p["name"],
        "version": p["version"],
        "license": p.get("license"),
        "source": p.get("source"),
        "id": p["id"],
        "description": (p.get("description") or "")[:200],
    })
pkgs.sort(key=lambda x: (x["name"], x["version"]))

inventory = {
    "schema_version": 1,
    "format": "aurum-inventory",
    "spdx_like": True,
    "document_name": f"aurum-{version}",
    "created": date_utc,
    "creator": "scripts/generate_sbom.sh",
    "source_commit": commit,
    "package_count": len(pkgs),
    "packages": pkgs,
}
(out / "aurum-sbom-inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")

lines = [
    f"# Aurum SBOM inventory v{version}",
    "",
    f"- Created: `{date_utc}`",
    f"- Commit: `{commit}`",
    f"- Packages: **{len(pkgs)}**",
    "",
    "| Crate | Version | License | Source |",
    "|-------|---------|---------|--------|",
]
for p in pkgs:
    src = p["source"] or "path"
    if src.startswith("registry+"):
        src = "crates.io"
    lic = (p["license"] or "?").replace("|", "/")
    lines.append(f"| {p['name']} | {p['version']} | {lic} | {src} |")
(out / "aurum-sbom-inventory.md").write_text("\n".join(lines) + "\n")

# --- CycloneDX 1.5 (required release artifact, JOE-1859) ---
components = []
for p in pkgs:
    purl = None
    src = p["source"] or ""
    if "crates.io" in src or src.startswith("registry+https://github.com/rust-lang/crates.io-index"):
        purl = f"pkg:cargo/{p['name']}@{p['version']}"
    comp = {
        "type": "library",
        "name": p["name"],
        "version": p["version"],
        "bom-ref": p["id"],
    }
    if p.get("license"):
        # SPDX compound expressions and "OR"/"/" go in name; simple IDs in id.
        lic = p["license"]
        simple = lic.replace("-", "").replace(".", "").isalnum() and " " not in lic and "/" not in lic and "OR" not in lic
        comp["licenses"] = [
            {"license": {"id": lic} if simple else {"name": lic}}
        ]
    if purl:
        comp["purl"] = purl
    if p.get("description"):
        comp["description"] = p["description"]
    components.append(comp)

cdx = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "serialNumber": f"urn:uuid:{uuid.uuid5(uuid.NAMESPACE_URL, f'aurum-{version}-{commit}')}",
    "version": 1,
    "metadata": {
        "timestamp": date_utc,
        "tools": {
            "components": [{
                "type": "application",
                "name": "generate_sbom.sh",
                "version": "1",
            }]
        },
        "component": {
            "type": "application",
            "name": "aurum",
            "version": version,
            "bom-ref": f"aurum@{version}",
            "description": "Local-first STT/TTS CLI and libraries",
        },
        "properties": [
            {"name": "aurum:source_commit", "value": commit},
        ],
    },
    "components": components,
}
(out / "aurum.cdx.json").write_text(json.dumps(cdx, indent=2) + "\n")

# --- SPDX 2.3 lite (required release artifact, JOE-1859) ---
spdx_packages = [{
    "SPDXID": "SPDXRef-Package-aurum",
    "name": "aurum",
    "downloadLocation": "NOASSERTION",
    "filesAnalyzed": False,
    "versionInfo": version,
    "supplier": "NOASSERTION",
    "externalRefs": [{
        "referenceCategory": "OTHER",
        "referenceType": "aurum-source-commit",
        "referenceLocator": commit,
    }],
}]
for i, p in enumerate(pkgs):
    spdx_packages.append({
        "SPDXID": f"SPDXRef-Package-{i}",
        "name": p["name"],
        "downloadLocation": p["source"] or "NOASSERTION",
        "filesAnalyzed": False,
        "versionInfo": p["version"],
        "licenseConcluded": p["license"] or "NOASSERTION",
        "licenseDeclared": p["license"] or "NOASSERTION",
        "copyrightText": "NOASSERTION",
    })

spdx_doc = {
    "spdxVersion": "SPDX-2.3",
    "dataLicense": "CC0-1.0",
    "SPDXID": "SPDXRef-DOCUMENT",
    "name": f"aurum-{version}",
    "documentNamespace": f"https://github.com/joe-broadhead/aurum/sbom/{version}/{commit[:12]}",
    "creationInfo": {
        "created": date_utc,
        "creators": ["Tool: scripts/generate_sbom.sh"],
    },
    "packages": spdx_packages,
    "relationships": [{
        "spdxElementId": "SPDXRef-DOCUMENT",
        "relationshipType": "DESCRIBES",
        "relatedSpdxElement": "SPDXRef-Package-aurum",
    }],
}
(out / "aurum.spdx.json").write_text(json.dumps(spdx_doc, indent=2) + "\n")

# Integrity digests for required formal documents
manifest = {"schema_version": 1, "files": {}}
for name in ("aurum.cdx.json", "aurum.spdx.json", "aurum-sbom-inventory.json"):
    data = (out / name).read_bytes()
    manifest["files"][name] = {
        "sha256": hashlib.sha256(data).hexdigest(),
        "bytes": len(data),
    }
(out / "SBOM_MANIFEST.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(f"Wrote formal SBOMs + inventory ({len(pkgs)} packages) → {out}")
PY

if command -v cargo-cyclonedx >/dev/null 2>&1; then
  cargo cyclonedx -f json --manifest-path Cargo.toml \
    --output-cdx "${OUT_DIR}/aurum.cdx.tool.json" 2>/dev/null \
    && echo "Optional tool CycloneDX also written (aurum.cdx.tool.json)." \
    || echo "cargo-cyclonedx present but failed; required aurum.cdx.json still valid."
else
  echo "cargo-cyclonedx not installed; using generated aurum.cdx.json (required)."
fi

cat > "${OUT_DIR}/native-components.md" <<EOF
# Native / runtime components

| Component | Role | Notes |
|-----------|------|-------|
| whisper-rs / whisper.cpp | Local STT | Native code; platform-specific (Metal on macOS) |
| ort (ONNX Runtime) | Local TTS | Vendor prebuilts via \`download-binaries\` when feature \`tts\` |
| ffmpeg | STT decode | **System** dependency; not bundled |
| misaki-rs | TTS G2P | MIT; default TTS path |

Source commit: \`${commit}\`
Version: \`${version}\`

Formal SBOMs in this directory: \`aurum.cdx.json\` (CycloneDX 1.5), \`aurum.spdx.json\` (SPDX 2.3 lite).
EOF

# Fail closed if formal artifacts missing
for req in aurum.cdx.json aurum.spdx.json aurum-sbom-inventory.json SBOM_MANIFEST.json; do
  if [ ! -f "${OUT_DIR}/${req}" ]; then
    echo "ERROR: required SBOM artifact missing: ${req}" >&2
    exit 1
  fi
done

echo "SBOM outputs in ${OUT_DIR}"
