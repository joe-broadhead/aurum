#!/usr/bin/env bash
# Module-scoped branch coverage for trust boundaries (JOE-1888 / JOE-1655).
#
# Produces per-module reports for domain, dto, output, cleanup, providers,
# model, error, secret, cancel, runtime, audio — not a single vanity aggregate.
#
# Usage:
#   ./scripts/coverage_trust.sh [OUT_DIR]
#
# Requires: cargo-llvm-cov (installed by script if missing), llvm-tools-preview
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${1:-dist/coverage-trust}"
mkdir -p "${OUT}"

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "Installing cargo-llvm-cov 0.6.16 (pinned)..."
  cargo install cargo-llvm-cov --locked --version 0.6.16 --force
fi

rustup component add llvm-tools-preview 2>/dev/null || true

echo "== cargo llvm-cov (aurum-core, no-default-features) =="
# summary-only for the human log
cargo llvm-cov -p aurum-core --lib --no-default-features --locked \
  --summary-only 2>&1 | tee "${OUT}/llvm-cov.log"

# LCOV for archival / module extraction
cargo llvm-cov -p aurum-core --lib --no-default-features --locked \
  --lcov --output-path "${OUT}/lcov.info"

# Try branch-aware export when tool supports it (best-effort).
cargo llvm-cov -p aurum-core --lib --no-default-features --locked \
  --branch --lcov --output-path "${OUT}/lcov-branch.info" 2>/dev/null \
  || cp "${OUT}/lcov.info" "${OUT}/lcov-branch.info"

python3 - <<'PY' "${OUT}"
import sys
from pathlib import Path

out = Path(sys.argv[1])
lcov_path = out / "lcov-branch.info"
if not lcov_path.is_file():
    lcov_path = out / "lcov.info"

mods = {
    "domain": "crates/aurum-core/src/domain.rs",
    "dto": "crates/aurum-core/src/dto.rs",
    "output": "crates/aurum-core/src/output/",
    "cleanup": "crates/aurum-core/src/cleanup/",
    "providers": "crates/aurum-core/src/providers/",
    "model": "crates/aurum-core/src/model/",
    "error": "crates/aurum-core/src/error.rs",
    "secret": "crates/aurum-core/src/secret.rs",
    "cancel": "crates/aurum-core/src/cancel.rs",
    "runtime": "crates/aurum-core/src/runtime/",
    "audio_parse": "crates/aurum-core/src/audio/",
}

stats = {k: {"LF": 0, "LH": 0, "BRF": 0, "BRH": 0} for k in mods}
current_file = None
if lcov_path.is_file():
    for line in lcov_path.read_text(errors="replace").splitlines():
        if line.startswith("SF:"):
            current_file = line[3:].replace("\\", "/")
        elif line.startswith("end_of_record"):
            current_file = None
        elif current_file:
            for key in ("LF", "LH", "BRF", "BRH"):
                if line.startswith(key + ":"):
                    try:
                        n = int(line.split(":", 1)[1].strip())
                    except ValueError:
                        continue
                    for name, frag in mods.items():
                        if frag in current_file:
                            stats[name][key] += n

report = []
report.append("# Trust-boundary coverage report (JOE-1888)")
report.append("")
report.append("Branch-oriented module report for release evidence.")
report.append("Policy: see docs/operations/qe-depth.md — **no single global % gate**.")
report.append("")
report.append("| Module | Line hit/found | Line % | Branch hit/found | Branch % |")
report.append("|--------|----------------:|-------:|------------------:|---------:|")
for name in mods:
    s = stats[name]
    lf, lh = s["LF"], s["LH"]
    brf, brh = s["BRF"], s["BRH"]
    lp = f"{(100.0 * lh / lf):.1f}" if lf else "n/a"
    bp = f"{(100.0 * brh / brf):.1f}" if brf else "n/a"
    report.append(f"| `{name}` | {lh}/{lf} | {lp} | {brh}/{brf} | {bp} |")

report += [
    "",
    "## Floors / trends (policy)",
    "",
    "| Module class | Soft floor (line) | Soft floor (branch) | Action if below |",
    "|--------------|------------------:|--------------------:|-----------------|",
    "| domain / dto / output / cleanup rules | 70% | 50% when measured | add unit tests before 1.0 RC |",
    "| model digest pins / error mapping | 80% | 60% when measured | block RC exit |",
    "| providers segment validation | 70% | 50% when measured | add adversarial seeds |",
    "| native whisper/ORT paths | n/a (not llvm-cov pure) | n/a | integration + sanitizer matrix |",
    "",
    "Soft floors are **trend expectations**, not vanity PR blockers in 0.0.x.",
    "Release evidence retains this report as a CI artifact.",
    "",
]
text = "\n".join(report)
(out / "TRUST_COVERAGE.md").write_text(text)
print(text)
PY

echo "Wrote ${OUT}/TRUST_COVERAGE.md"
echo "coverage_trust.sh OK"
