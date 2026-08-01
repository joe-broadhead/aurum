#!/usr/bin/env python3
"""Score full-file hypotheses against filled gold clips."""
from __future__ import annotations
import json, re, sys
from pathlib import Path
from jiwer import wer

ROOT = Path(__file__).resolve().parent
HYP_DIR = ROOT.parent / "hypotheses"
sys.path.insert(0, str(ROOT.parent))
from build_gold_subset import extract_hyp_span, norm  # type: ignore

def main() -> int:
    clips = sorted((ROOT / "clips").glob("clip_*"))
    missing = [c.name for c in clips if not (c / "gold.txt").read_text().strip()]
    if missing:
        print("Gold not filled for:", ", ".join(missing))
        print("Listen to clips/*/audio.wav and write verbatim text into gold.txt")
        return 1
    summary = []
    for p in sorted(HYP_DIR.glob("*.txt")):
        full = p.read_text()
        wers = []
        for c in clips:
            g = norm((c / "gold.txt").read_text())
            anchor = (c / "youtube_draft.txt").read_text()
            span = extract_hyp_span(full, anchor) or extract_hyp_span(full, g)
            if not span:
                continue
            wers.append(float(wer(g, norm(span))))
        if wers:
            summary.append((sum(wers) / len(wers), p.name, len(wers)))
    summary.sort()
    print(f"{'mean_wer':>10}  n  file")
    for m, name, n in summary:
        print(f"{100*m:9.2f}%  {n:2d}  {name}")
    (ROOT / "gold_scores.json").write_text(
        json.dumps([{"file": n, "mean_wer": m, "n_clips": k} for m, n, k in summary], indent=2) + "\n"
    )
    print("\nWrote gold_scores.json")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
