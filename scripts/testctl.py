#!/usr/bin/env python3
"""Run declared test segments. See SPEC.md.

No third-party imports. Python 3.11+ (tomllib).
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
from pathlib import Path

EXIT_OK, EXIT_FAIL, EXIT_USAGE, EXIT_BUDGET, EXIT_SKIP, EXIT_TIMEOUT = 0, 1, 2, 3, 4, 5
_os_name = {"darwin": "macos", "linux": "linux"}.get(sys.platform, sys.platform)
HOST = f"{_os_name}-{os.uname().machine} {os.cpu_count()}c"


def die(msg: str, code: int = EXIT_USAGE) -> "NoReturn":
    print(f"testctl: {msg}", file=sys.stderr)
    raise SystemExit(code)


def repo_root(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit).resolve()
    return Path(__file__).resolve().parent.parent


def load(root: Path) -> dict:
    path = root / "test-segments.toml"
    if not path.exists():
        die(f"no test-segments.toml in {root}", EXIT_USAGE)
    with path.open("rb") as fh:
        data = tomllib.load(fh)
    meta = data.get("meta") or {}
    segs = data.get("segment") or []
    if not segs:
        die("manifest has no [[segment]]", EXIT_USAGE)
    for key in ("reference_host", "universe"):
        if key not in meta:
            die(f"meta.{key} is required", EXIT_USAGE)
    seen = set()
    for s in segs:
        sid = s.get("id")
        if not sid or sid in seen:
            die(f"segment id missing or duplicated: {sid!r}", EXIT_USAGE)
        seen.add(sid)
        if s.get("kind") not in ("test", "static"):
            die(f"{sid}: kind must be test or static", EXIT_USAGE)
        if s.get("ignored", "exclude") not in ("exclude", "only", "include"):
            die(f"{sid}: ignored must be exclude, only, or include", EXIT_USAGE)
        for key in ("run", "list"):
            if key in s and (not isinstance(s[key], list) or not all(isinstance(x, str) for x in s[key])):
                die(f"{sid}: {key} must be an argv array of strings", EXIT_USAGE)
        if s["kind"] == "test" and "list" not in s:
            die(f"{sid}: test segments need list for --verify", EXIT_USAGE)
        if not isinstance(s.get("expected_s"), (int, float)):
            die(f"{sid}: expected_s must be a number", EXIT_USAGE)
    return {"meta": meta, "segments": segs}


def selected(segs: list[dict], args) -> list[dict]:
    by_id = {s["id"]: s for s in segs}
    if args.segment:
        missing = [x for x in args.segment if x not in by_id]
        if missing:
            die(f"unknown segment(s): {', '.join(missing)}", EXIT_USAGE)
        chosen = [by_id[x] for x in args.segment]
    elif args.graphical and args.full:
        chosen = [s for s in segs if not s.get("nightly", False)]
    elif args.graphical:
        chosen = [s for s in segs if s.get("graphical", False)]
    elif args.full:
        chosen = [s for s in segs if not s.get("nightly", False) and not s.get("graphical", False)]
    else:
        chosen = [s for s in segs if s.get("default", False)]
    if args.nightly:
        chosen = [s for s in segs if s in chosen or s.get("nightly", False)]
    return chosen


def capability_missing(seg: dict, args) -> str:
    """Return a reason string when a requirement cannot be met."""
    for req in seg.get("requires", []):
        kind, _, name = req.partition(":")
        if kind == "display":
            if not (args.graphical or args.allow_display):
                return "display (pass --graphical or --allow-display)"
        elif kind == "network":
            if args.offline:
                return "network (--offline)"
        elif kind == "binary":
            if not shutil.which(name):
                return f"binary:{name} not on PATH"
        elif kind == "toolchain":
            if not toolchain_present(name):
                return f"toolchain {name} not installed (rustup toolchain install {name})"
        elif kind in ("artifacts", "secrets"):
            if not os.environ.get(name):
                return f"{kind}:{name} is not set in the environment"
        elif kind == "slow":
            continue
        else:
            return f"unknown requirement {req!r}"
    return ""


def toolchain_present(name: str) -> bool:
    try:
        out = subprocess.run(["rustup", "toolchain", "list"], capture_output=True, text=True, timeout=20)
    except (OSError, subprocess.SubprocessError):
        return False
    return any(line.split("-")[0].strip() == name or line.strip() == name for line in out.stdout.splitlines() if line.strip())


def json_budget(chosen: list[dict], args) -> tuple[list[dict], list[dict], bool]:
    """Drop the most expensive optional segments until the sum fits."""
    if not args.budget:
        return chosen, [], True
    explicit = set(args.segment or [])
    total = sum(s["expected_s"] for s in chosen)
    dropped: list[dict] = []
    keep = list(chosen)
    for seg in sorted(chosen, key=lambda s: s["expected_s"], reverse=True):
        if total <= args.budget:
            break
        if seg["id"] in explicit:
            continue
        keep.remove(seg)
        dropped.append(seg)
        total -= seg["expected_s"]
    return keep, dropped, total <= args.budget


def cmd_list(manifest: dict, args) -> int:
    segs = manifest["segments"]
    if args.json:
        print(json.dumps({"meta": manifest["meta"], "segment": segs}, indent=2))
        return EXIT_OK
    print(f"host: {manifest['meta']['reference_host']}  (this machine: {HOST})")
    print(f"{'id':22} {'kind':7} {'est':>7}  {'ignored':9} {'flags':18} requires")
    for s in segs:
        flags = ",".join(
            x for x, on in (("default", s.get("default")), ("graphical", s.get("graphical")), ("nightly", s.get("nightly"))) if on
        )
        print(
            f"{s['id']:22} {s['kind']:7} {s['expected_s']:>6}s  {s.get('ignored', 'exclude'):9} {flags:18} "
            f"{','.join(s.get('requires', []))}"
        )
    return EXIT_OK


def cmd_plan(manifest: dict, args) -> int:
    chosen = selected(manifest["segments"], args)
    keep, dropped, fits = json_budget(chosen, args)
    print(f"selected ({len(keep)}):")
    for s in keep:
        reason = capability_missing(s, args)
        state = f"SKIP {reason}" if reason else "run"
        print(f"  {s['id']:22} {s['expected_s']:>6}s  {state}")
    if dropped:
        print(f"dropped for budget ({len(dropped)}): {', '.join(s['id'] for s in dropped)}")
    total = sum(s["expected_s"] for s in keep)
    print(f"estimated total: {total}s" + (f" of {args.budget}s budget" if args.budget else ""))
    if not fits:
        print("CANNOT_VERIFY: the named segments exceed the budget", file=sys.stderr)
        return EXIT_BUDGET
    return EXIT_OK


def run_segment(seg: dict, root: Path, timeout: float) -> tuple[int, float, str]:
    started = time.monotonic()
    try:
        proc = subprocess.run(seg["run"], cwd=str(root), timeout=max(1, timeout))
        return proc.returncode, time.monotonic() - started, ""
    except subprocess.TimeoutExpired:
        return EXIT_TIMEOUT, time.monotonic() - started, f"timed out after {int(timeout)}s"
    except FileNotFoundError as exc:
        return 127, time.monotonic() - started, f"command not found: {exc}"


def cmd_run(manifest: dict, args, root: Path, mode: str) -> int:
    chosen = selected(manifest["segments"], args)
    keep, dropped, fits = json_budget(chosen, args)
    if not fits:
        print("CANNOT_VERIFY: the named segments exceed the budget", file=sys.stderr)
        return EXIT_BUDGET
    if dropped:
        print(f"dropped for budget: {', '.join(s['id'] for s in dropped)}")
    remaining = float(args.budget) if args.budget else None
    failed, skipped, results = [], [], {}
    for seg in keep:
        reason = capability_missing(seg, args)
        if reason:
            print(f"SKIP {seg['id']}: {reason}", file=sys.stderr)
            skipped.append((seg["id"], reason))
            continue
        timeout = max(120.0, seg["expected_s"] * 3 + 60)
        if remaining is not None:
            timeout = min(timeout, remaining)
            if timeout <= 0:
                skipped.append((seg["id"], "budget exhausted"))
                continue
        print(f"=== {seg['id']}: {' '.join(seg['run'])}", flush=True)
        rc, seconds, note = run_segment(seg, root, timeout)
        print(f"--- {seg['id']}: exit={rc} {seconds:.1f}s {note}", flush=True)
        results[seg["id"]] = {"duration_s": round(seconds, 1), "exit": rc}
        if remaining is not None:
            remaining -= seconds
        if rc != 0:
            failed.append((seg["id"], rc))
    if mode == "measure":
        lock = root / "test-segments.lock.json"
        payload = {"host": HOST, "measured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "segments": results}
        lock.write_text(json.dumps(payload, indent=2) + "\n")
        print(f"wrote {lock}")
        return EXIT_OK
    for sid, rc in failed:
        print(f"FAIL {sid} (exit {rc})", file=sys.stderr)
    for sid, reason in skipped:
        print(f"SKIP {sid}: {reason}", file=sys.stderr)
    if failed:
        return EXIT_FAIL
    if skipped and len(skipped) == len(keep):
        return EXIT_SKIP
    return EXIT_OK


def parse_tests(payload: str) -> dict[str, bool | None]:
    """Parse nextest JSON, libtest listing, or one-name-per-line output.

    The value is the ignored flag: True, False, or None when the runner cannot
    report it (libtest, file-level listing).
    """
    text = payload.strip()
    if not text:
        return {}
    try:
        data = json.loads(text)
    except json.JSONDecodeError:
        data = None
    ids: dict[str, bool | None] = {}
    if data is not None:
        def walk(node):
            if isinstance(node, dict):
                if isinstance(node.get("testcases"), dict):
                    binary = node.get("binary-id") or node.get("binary_id") or node.get("name") or "?"
                    for name, meta in node["testcases"].items():
                        ignored = bool(meta.get("ignored", False)) if isinstance(meta, dict) else False
                        ids[f"{binary}::{name}"] = ignored
                for value in node.values():
                    walk(value)
            elif isinstance(node, list):
                for value in node:
                    walk(value)
        walk(data)
        if ids:
            return ids
    for line in text.splitlines():
        line = line.strip()
        if line.endswith(": test"):
            ids[line[: -len(": test")]] = None
        elif line and not line.startswith(("{", "[", "#")):
            ids[line] = None
    return ids


def list_segment(seg: dict, root: Path, timeout: float = 300) -> dict[str, bool | None]:
    try:
        proc = subprocess.run(seg["list"], cwd=str(root), capture_output=True, text=True, timeout=timeout)
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return {}
    if proc.returncode != 0:
        return {}
    return parse_tests(proc.stdout)


def claims(seg: dict, ignored: bool | None) -> bool:
    """Whether a segment claims a test with this ignored flag."""
    mode = seg.get("ignored", "exclude")
    if mode == "include":
        return True
    if mode == "only":
        return ignored is not False
    return ignored is not True


def cmd_verify(manifest: dict, args, root: Path) -> int:
    universe = parse_tests(
        subprocess.run(manifest["meta"]["universe"], cwd=str(root), capture_output=True, text=True).stdout
    )
    if not universe:
        print("verify: the universe command returned no tests", file=sys.stderr)
        return EXIT_USAGE
    ignored_total = 0
    seen: dict[str, str] = {}
    seen_ignored: dict[str, bool | None] = {}
    duplicates: list[str] = []
    wrong_mode: list[str] = []
    for seg in manifest["segments"]:
        if seg["kind"] != "test":
            continue
        mode = seg.get("ignored", "exclude")
        listed = list_segment(seg, root)
        claimed = 0
        for test, flag in listed.items():
            if not claims(seg, flag):
                continue
            claimed += 1
            if test in seen:
                duplicates.append(f"{test} ({seen[test]} and {seg['id']})")
            else:
                seen[test] = seg["id"]
                seen_ignored[test] = flag
                if flag:
                    ignored_total += 1
        if listed and mode == "only" and claimed == 0:
            wrong_mode.append(f"{seg['id']} claims ignored=only but lists no ignored tests")
        if listed and mode == "exclude" and claimed == 0:
            wrong_mode.append(f"{seg['id']} claims ignored=exclude but lists only ignored tests")
    missing = sorted(set(universe) - set(seen))
    unexpected = sorted(set(seen) - set(universe))
    print(f"universe: {len(universe)} tests ({ignored_total} ignored)")
    print(
        f"declared: {len(seen)} tests across "
        f"{sum(1 for s in manifest['segments'] if s['kind'] == 'test')} segments"
    )
    for label, items in (("missing", missing), ("duplicate", duplicates), ("wrong-mode", wrong_mode), ("unexpected", unexpected)):
        for item in items[:50]:
            print(f"{label}: {item}")
        if len(items) > 50:
            print(f"{label}: ... {len(items) - 50} more")
    if missing or duplicates or unexpected or wrong_mode:
        print(
            f"verify: FAIL ({len(missing)} missing, {len(duplicates)} duplicate, "
            f"{len(wrong_mode)} wrong-mode, {len(unexpected)} unexpected)"
        )
        return EXIT_FAIL
    print("verify: PASS (every test in exactly one segment)")
    return EXIT_OK


def main() -> int:
    ap = argparse.ArgumentParser(prog="test.sh", description="Run declared test segments.")
    ap.add_argument("--repo", help="repository root (default: parent of scripts/)")
    ap.add_argument("--list", action="store_true", help="print the manifest")
    ap.add_argument("--json", action="store_true", help="machine-readable output for --list")
    ap.add_argument("--segment", action="append", metavar="ID", help="run exactly this segment (repeatable)")
    ap.add_argument("--full", action="store_true", help="all non-nightly segments (graphical excluded)")
    ap.add_argument("--graphical", action="store_true", help="graphical segments only, or include them with --full")
    ap.add_argument("--nightly", action="store_true", help="also include nightly segments")
    ap.add_argument("--allow-display", action="store_true", help="satisfy the display capability")
    ap.add_argument("--offline", action="store_true", help="fail network requirements")
    ap.add_argument("--budget", type=int, metavar="SECONDS", help="drop optional segments until the estimate fits")
    ap.add_argument("--dry-run", action="store_true", help="print the resolved set and costs")
    ap.add_argument("--verify", action="store_true", help="prove every test belongs to exactly one segment")
    ap.add_argument("--measure", action="store_true", help="run and record durations in test-segments.lock.json")
    args = ap.parse_args()

    root = repo_root(args.repo)
    manifest = load(root)

    if args.list:
        return cmd_list(manifest, args)
    if args.dry_run:
        return cmd_plan(manifest, args)
    if args.verify:
        return cmd_verify(manifest, args, root)
    if args.measure:
        return cmd_run(manifest, args, root, "measure")
    return cmd_run(manifest, args, root, "run")


if __name__ == "__main__":
    sys.exit(main())
