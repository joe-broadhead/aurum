#!/usr/bin/env bash
# Everything that counts as a test in this repository: nextest's universe plus
# doctests and the Python runner and eval-helper regressions, which nextest does
# not run.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo nextest list --workspace --locked --message-format json | python3 -c '
import json, sys

data = json.load(sys.stdin)

def walk(node):
    if isinstance(node, dict):
        if isinstance(node.get("testcases"), dict):
            binary = node.get("binary-id") or node.get("name") or "?"
            for name in node["testcases"]:
                print(f"{binary}::{name}")
        for value in node.values():
            walk(value)
    elif isinstance(node, list):
        for value in node:
            walk(value)

walk(data)
'
cargo test --workspace --doc --locked -- --list 2>/dev/null | sed -n 's/^\(.*\): test$/\1/p'
python3 scripts/test_testctl.py --list
python3 scripts/eval/tests/list_tests.py
