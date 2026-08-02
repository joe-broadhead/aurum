#!/usr/bin/env python3
"""Compare an STT observatory (or legacy) report to a committed budget (JOE-2216).

Exit 0 on pass, 1 on budget violation, 2 on usage/parse errors.
Does not load audio or print hypotheses.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as f:
        return json.load(f)


def mean_wer_from_report(report: dict[str, Any]) -> float:
    if "mean_wer" in report:
        return float(report["mean_wer"])
    scores = report.get("scores") or report.get("stt_scores") or []
    if not scores:
        return 0.0
    return sum(float(s.get("wer", 0.0)) for s in scores) / len(scores)


def silence_fp_from_report(report: dict[str, Any]) -> int:
    if "silence_false_positives" in report:
        return int(report["silence_false_positives"])
    scores = report.get("scores") or report.get("stt_scores") or []
    return sum(1 for s in scores if s.get("silence_false_positive"))


def mean_rep_from_report(report: dict[str, Any]) -> float:
    if "mean_repetition_ratio" in report:
        return float(report["mean_repetition_ratio"])
    scores = report.get("scores") or report.get("stt_scores") or []
    reps = [float(s["repetition_ratio"]) for s in scores if s.get("repetition_ratio") is not None]
    if not reps:
        return 0.0
    return sum(reps) / len(reps)


def scenario_means(report: dict[str, Any]) -> dict[str, float]:
    if "scenario_mean_wer" in report and isinstance(report["scenario_mean_wer"], dict):
        return {k: float(v) for k, v in report["scenario_mean_wer"].items()}
    # Legacy: group by first tag
    scores = report.get("scores") or report.get("stt_scores") or []
    buckets: dict[str, list[float]] = {}
    for s in scores:
        tags = s.get("tags") or []
        key = tags[0] if tags else "untagged"
        buckets.setdefault(key, []).append(float(s.get("wer", 0.0)))
    return {k: sum(v) / len(v) for k, v in buckets.items()}


def allowed_mean_wer(baseline: float, rel: float, abs_pts: float) -> float:
    return max(baseline * (1.0 + rel), baseline + abs_pts)


def compare(report: dict[str, Any], budget: dict[str, Any]) -> tuple[bool, list[str]]:
    findings: list[str] = []
    model = report.get("model") or (report.get("identity") or {}).get("model_id")
    if model and budget.get("model") and model != budget["model"]:
        findings.append(f"FAIL model_mismatch: report={model} budget={budget['model']}")

    cand = mean_wer_from_report(report)
    base = float(budget["baseline_mean_wer"])
    rel = float(budget.get("max_relative_wer", 0.10))
    abs_pts = float(budget.get("max_absolute_wer_points", 1.0))
    allowed = allowed_mean_wer(base, rel, abs_pts)
    if cand > allowed + 1e-12:
        findings.append(
            f"FAIL aggregate_wer_regression: mean_wer={cand:.4f} allowed={allowed:.4f} "
            f"(baseline={base:.4f}, rel={rel:.0%}, abs=+{abs_pts})"
        )

    sfp = silence_fp_from_report(report)
    max_sfp = int(budget.get("max_silence_false_positives", 0))
    if sfp > max_sfp:
        findings.append(f"FAIL silence_false_positive: {sfp} > max {max_sfp}")

    rep = mean_rep_from_report(report)
    max_rep = float(budget.get("max_mean_repetition_ratio", 0.35))
    if rep > max_rep + 1e-12:
        findings.append(f"FAIL repetition_degeneration: {rep:.4f} > max {max_rep:.4f}")

    scen_rel = float(budget.get("max_scenario_relative_wer", 0.15))
    scen_base = budget.get("scenario_baseline_wer") or {}
    scen_cand = scenario_means(report)
    for name, b in scen_base.items():
        if name in scen_cand:
            allowed_s = float(b) * (1.0 + scen_rel)
            if scen_cand[name] > allowed_s + 1e-12:
                findings.append(
                    f"FAIL scenario_wer_regression: {name}={scen_cand[name]:.4f} "
                    f"allowed={allowed_s:.4f} (baseline={float(b):.4f})"
                )

    if not findings:
        findings.append(
            f"PASS ok: mean_wer={cand:.4f} allowed={allowed:.4f} silence_fp={sfp} rep={rep:.4f}"
        )
        return True, findings
    return False, findings


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--report", type=Path, required=True, help="Candidate JSON report")
    p.add_argument("--budget", type=Path, required=True, help="Committed budget JSON")
    p.add_argument("--json", action="store_true", help="Emit findings as JSON")
    args = p.parse_args()

    try:
        report = load_json(args.report)
        budget = load_json(args.budget)
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
