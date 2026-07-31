#!/usr/bin/env bash
# Generate release-oriented dependency inventory / SBOM-lite (JOE-1635).
# Produces SPDX-ish JSON via cargo metadata + a human inventory.
# Optional: cargo-cyclonedx when installed (cargo install cargo-cyclonedx).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${1:-dist/sbom}"
mkdir -p "${OUT_DIR}"

version="$(tr -d '[:space:]' < VERSION)"
commit="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
date_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo "Generating SBOM inventory for v${version} @ ${commit}"

# Machine-readable cargo metadata (full graph).
cargo metadata --format-version 1 --locked > "${OUT_DIR}/cargo-metadata.json"

# Flattened crate list for human review.
python3 - <<'PY' "${OUT_DIR}" "${version}" "${commit}" "${date_utc}"
import json, sys
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
    })
pkgs.sort(key=lambda x: (x["name"], x["version"]))
doc = {
    "schema_version": 1,
    "spdx_like": True,
    "document_name": f"aurum-{version}",
    "created": date_utc,
    "creator": "scripts/generate_sbom.sh",
    "source_commit": commit,
    "package_count": len(pkgs),
    "packages": pkgs,
}
(out / "aurum-sbom-inventory.json").write_text(json.dumps(doc, indent=2) + "\n")
# Markdown summary
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
print(f"Wrote {out / 'aurum-sbom-inventory.json'} ({len(pkgs)} packages)")
PY

# Optional CycloneDX if tool present.
if command -v cargo-cyclonedx >/dev/null 2>&1; then
  cargo cyclonedx -f json --manifest-path Cargo.toml \
    --output-cdx "${OUT_DIR}/aurum.cdx.json" || true
  echo "CycloneDX written (if supported by tool version)."
else
  echo "cargo-cyclonedx not installed; inventory JSON is the required artifact."
fi

# Record native runtime notes for operators.
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
EOF

echo "SBOM outputs in ${OUT_DIR}"
