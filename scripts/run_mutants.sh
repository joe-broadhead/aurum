#!/usr/bin/env bash
# Targeted mutation testing for critical modules (JOE-1886 / JOE-1655).
#
# Uses cargo-mutants with a tight file allowlist and time caps.
# Smoke mode is for PR CI; full mode is for scheduled/local campaigns.
#
# Usage:
#   ./scripts/run_mutants.sh --smoke
#   ./scripts/run_mutants.sh --full
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="smoke"
while [ $# -gt 0 ]; do
  case "$1" in
    --smoke) MODE="smoke"; shift ;;
    --full) MODE="full"; shift ;;
    -h|--help) sed -n '1,20p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if ! cargo mutants --version >/dev/null 2>&1; then
  echo "Installing cargo-mutants 25.0.0 (pinned)..."
  cargo install cargo-mutants --locked --version 25.0.0 --force
fi

OUT="dist/mutants"
mkdir -p "${OUT}"

# Prove unmutated tree is green first (avoids cargo-mutants early baseline timeout
# while rebuilding whisper from a clean temp tree).
echo "== baseline: cargo test -p aurum-core --lib --no-default-features =="
cargo test -p aurum-core --lib --no-default-features --locked -- domain:: dto::

if [ "${MODE}" = "smoke" ]; then
  TEST_TIMEOUT="${AURUM_MUTANTS_TIMEOUT:-180}"
  BUILD_TIMEOUT="${AURUM_MUTANTS_BUILD_TIMEOUT:-900}"
  FILES=(
    --file 'crates/aurum-core/src/domain.rs'
  )
  # Shard keeps PR bounded once the copy tree is warm.
  EXTRA=(--timeout "${TEST_TIMEOUT}" --build-timeout "${BUILD_TIMEOUT}" --shard 1/2 --jobs 1)
  echo "== cargo-mutants smoke (domain.rs, shard 1/2) =="
else
  TEST_TIMEOUT="${AURUM_MUTANTS_TIMEOUT:-300}"
  BUILD_TIMEOUT="${AURUM_MUTANTS_BUILD_TIMEOUT:-1200}"
  FILES=(
    --file 'crates/aurum-core/src/domain.rs'
    --file 'crates/aurum-core/src/dto.rs'
    --file 'crates/aurum-core/src/error.rs'
    --file 'crates/aurum-core/src/cleanup/rules.rs'
    --file 'crates/aurum-core/src/providers/mod.rs'
    --file 'crates/aurum-core/src/model/mod.rs'
  )
  EXTRA=(--timeout "${TEST_TIMEOUT}" --build-timeout "${BUILD_TIMEOUT}" --jobs 2)
  echo "== cargo-mutants full =="
fi

set +e
# --baseline=skip: we already ran green tests above.
# Test args after -- limit to lib + STT-only features.
cargo mutants \
  --package aurum-core \
  --output "${OUT}" \
  --baseline=skip \
  "${FILES[@]}" \
  "${EXTRA[@]}" \
  -- --lib --no-default-features --locked \
  2>&1 | tee "${OUT}/mutants.log"
rc=${PIPESTATUS[0]}
set -e

python3 - <<'PY' "${OUT}" "${MODE}" "${rc}"
import sys
from pathlib import Path
out, mode, rc = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
log = (out / "mutants.log").read_text(errors="replace") if (out / "mutants.log").is_file() else ""
mout = out / "mutants.out"
outcomes = ""
if mout.is_dir():
    for name in ("outcomes.json", "caught.txt", "missed.txt", "unviable.txt", "timeout.txt"):
        p = mout / name
        if p.is_file():
            outcomes += f"\n### {name}\n```\n{p.read_text(errors='replace')[:6000]}\n```\n"

summary = out / "MUTANTS_SUMMARY.md"
lines = [
  "# Mutation test summary (JOE-1886)",
  "",
  f"- **mode:** {mode}",
  f"- **cargo-mutants exit:** {rc}",
  "",
  "## Scope",
  "",
  "Smoke: `domain.rs` shard. Full: domain/dto/error/cleanup/rules/providers/model.",
  "Tests: `cargo test -p aurum-core --lib --no-default-features`.",
  "",
  "## Policy",
  "",
  "- PR smoke must exit 0 (all examined mutants caught or unviable).",
  "- Before 1.0 RC: review survivors; uncaught digest/bounds/lifecycle/capability",
  "  mutants must be fixed or explicitly waived in the RC exit report.",
  "- Known acceptable survivors: pure Display/Debug formatting, log-only branches.",
  "",
  outcomes,
  "## Log tail",
  "",
  "```",
  "\n".join(log.strip().splitlines()[-100:]),
  "```",
  "",
]
summary.write_text("\n".join(lines))
print(summary.read_text())
sys.exit(rc)
PY

echo "run_mutants.sh finished (mode=${MODE})"
