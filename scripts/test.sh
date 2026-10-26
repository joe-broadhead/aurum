#!/usr/bin/env bash
# Thin entrypoint for the repository's declared test segments.
# Contract: GitHub issue #157. Implementation: testctl.py (vendored, no network).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$here/testctl.py" "$@"
