#!/usr/bin/env python3
"""Compare a performance report to a committed budget (JOE-2218).

Exit 0 pass (warnings allowed), 1 fail, 2 usage/parse error.
Does not load audio or print payloads.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


def load(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as f:
        return json.load(f)


def compare(report: dict[str, Any], budget: dict[str, Any]) -> tuple[bool, list[str]]:
    findings: list[str] = []
    hw = report.get("hardware") or {}
    profile = hw.get("profile_id") or report.get("profile") or report.get("hardware_profile")
    b_profile = budget.get("hardware_profile_id")
    if profile and b_profile and profile != b_profile:
        findings.append(f"FAIL hardware_mismatch: report={profile} budget={b_profile}")

    # Index scenarios
    scenarios = report.get("scenarios") or []
    by_id = {s.get("scenario_id"): s for s in scenarios if s.get("scenario_id")}
    # Legacy single-scenario reports
    if not by_id and report.get("kind"):
        sid = report.get("kind")
        by_id[sid] = {
            "scenario_id": sid,
            "p50_ms": report.get("p50_ms"),
            "p95_ms": report.get("p95_ms"),
            "rtf_p50": report.get("rtf_p50"),
            "peak_rss_bytes": report.get("peak_rss_bytes"),
        }

    for b in budget.get("scenarios") or []:
        sid = b["scenario_id"]
        cand = by_id.get(sid)
        if not cand:
            findings.append(f"FAIL missing_scenario: {sid}")
            continue
        p50 = float(cand.get("p50_ms") or 0)
        p95 = float(cand.get("p95_ms") or 0)
        base_p50 = float(b["baseline_p50_ms"])
        base_p95 = float(b["baseline_p95_ms"])
        warn = float(b.get("max_p50_relative_warn", 0.10))
        fail = float(b.get("max_p95_relative_fail", 0.15))
        if p50 > base_p50 * (1.0 + warn) + 1e-12:
            findings.append(
                f"WARN p50_regression: {sid} p50={p50:.1f} > {base_p50 * (1+warn):.1f}"
            )
        if p95 > base_p95 * (1.0 + fail) + 1e-12:
            findings.append(
                f"FAIL p95_regression: {sid} p95={p95:.1f} > {base_p95 * (1+fail):.1f}"
            )

    if not findings:
        findings.append("PASS ok")
        return True, findings
    failed = any(f.startswith("FAIL") for f in findings)
    return (not failed), findings


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--report", type=Path, required=True)
    p.add_argument("--budget", type=Path, required=True)
    p.add_argument("--json", action="store_true")
    args = p.parse_args()
    try:
        report = load(args.report)
        budget = load(args.budget)
    except (OSError, json.JSONDecodeError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    ok, findings = compare(report, budget)
    if args.json:
        print(json.dumps({"passed": ok, "findings": findings}, indent=2))
    else:
        for line in findings:
            print(line)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
