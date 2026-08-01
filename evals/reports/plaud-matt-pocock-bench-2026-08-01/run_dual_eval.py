#!/usr/bin/env python3
"""Dual-reference STT eval: score hyps against Plaud + YouTube.

Neither reference is human-verified oracle ground truth:
- Plaud = device/cloud transcript for the recording
- YouTube = auto-generated captions for the source video
  https://www.youtube.com/watch?v=n0VhIVtviC0 (Matt Pocock)

Scoring both reduces bias from either ASR pipeline alone.
Primary ranking key: mean WER across the two references.
"""
from __future__ import annotations

import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

from jiwer import cer, process_words, wer

ROOT = Path(__file__).resolve().parent
HYP_DIR = ROOT / "hypotheses"
OUT_JSON = ROOT / "eval_dual_results.json"
OUT_MD = ROOT / "EVAL_REPORT.md"
OUT_CSV = ROOT / "eval_dual_results.csv"

# Prefer full raw plaud; fall back
REF_PLAUD = ROOT / "plaud_transcript_full_raw.txt"
if not REF_PLAUD.exists():
    REF_PLAUD = ROOT / "plaud_ref.txt"
REF_YT = ROOT / "youtube_transcript_plain.txt"

# Optional wall times from prior benches
PRIOR_JSONL = [ROOT / "results.jsonl", ROOT / "results_openrouter.jsonl"]


def normalize(text: str) -> str:
    """WER-oriented normalization (case-fold, drop punct, collapse space)."""
    text = text.lower()
    text = re.sub(r"\[[^\]]*\]", " ", text)  # timestamps / speaker tags
    text = re.sub(r"speaker\s*\d+\s*:\s*", " ", text, flags=re.I)
    # common ASR variants
    text = text.replace("’", "'").replace("‘", "'").replace("“", '"').replace("”", '"')
    text = re.sub(r"\b(um|uh|erm)\b", " ", text)
    text = re.sub(r"[^\w\s']", " ", text)
    text = re.sub(r"\s+", " ", text).strip()
    return text


def score_pair(ref: str, hyp: str) -> dict:
    r, h = normalize(ref), normalize(hyp)
    if not r:
        return {"wer": None, "cer": None, "ref_words": 0, "hyp_words": len(h.split())}
    if not h:
        return {
            "wer": 1.0,
            "cer": 1.0,
            "ref_words": len(r.split()),
            "hyp_words": 0,
            "hits": 0,
            "substitutions": 0,
            "deletions": len(r.split()),
            "insertions": 0,
        }
    out = process_words(r, h)
    return {
        "wer": float(out.wer),
        "cer": float(cer(r, h)),
        "ref_words": int(out.references[0].__len__()) if hasattr(out, "references") else len(r.split()),
        "hyp_words": len(h.split()),
        "hits": int(out.hits),
        "substitutions": int(out.substitutions),
        "deletions": int(out.deletions),
        "insertions": int(out.insertions),
    }


def label_from_name(name: str) -> tuple[str, str, str]:
    """Return (system, model, label)."""
    stem = name.removesuffix(".txt")
    if stem.startswith("local_"):
        model = stem[len("local_") :]
        return "aurum-local", model, f"aurum/{model}"
    if stem.startswith("or_direct_"):
        model = stem[len("or_direct_") :].replace("_", "/", 1)
        # fix double path: openai_whisper-1 -> openai/whisper-1
        # we replaced only first _
        parts = stem[len("or_direct_") :].split("_", 1)
        if len(parts) == 2:
            model = f"{parts[0]}/{parts[1]}"
        else:
            model = parts[0]
        return "openrouter", model, f"openrouter/{model}"
    return "other", stem, stem


def load_wall_times() -> dict[str, float]:
    times: dict[str, float] = {}
    for path in PRIOR_JSONL:
        if not path.exists():
            continue
        for line in path.read_text().splitlines():
            if not line.strip():
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            model = row.get("model") or ""
            provider = row.get("provider") or ""
            if row.get("wall_s") is None:
                continue
            if provider == "local":
                times[f"aurum/{model}"] = float(row["wall_s"])
            elif "openrouter" in provider:
                times[f"openrouter/{model}"] = float(row["wall_s"])
    return times


def main() -> int:
    if not REF_PLAUD.exists() or not REF_YT.exists():
        print("missing refs", REF_PLAUD, REF_YT, file=sys.stderr)
        return 1
    plaud = REF_PLAUD.read_text()
    yt = REF_YT.read_text()
    wall = load_wall_times()

    # Cross-reference baseline (how far the two refs diverge)
    cross_yt_vs_plaud = score_pair(plaud, yt)
    cross_plaud_vs_yt = score_pair(yt, plaud)

    rows: list[dict] = []
    for path in sorted(HYP_DIR.glob("*.txt")):
        hyp = path.read_text()
        if not hyp.strip():
            continue
        system, model, label = label_from_name(path.name)
        vs_plaud = score_pair(plaud, hyp)
        vs_yt = score_pair(yt, hyp)
        mean_wer = None
        if vs_plaud["wer"] is not None and vs_yt["wer"] is not None:
            mean_wer = (vs_plaud["wer"] + vs_yt["wer"]) / 2.0
        mean_cer = None
        if vs_plaud["cer"] is not None and vs_yt["cer"] is not None:
            mean_cer = (vs_plaud["cer"] + vs_yt["cer"]) / 2.0
        len_ratio_plaud = (
            vs_plaud["hyp_words"] / vs_plaud["ref_words"] if vs_plaud["ref_words"] else None
        )
        len_ratio_yt = vs_yt["hyp_words"] / vs_yt["ref_words"] if vs_yt["ref_words"] else None
        row = {
            "label": label,
            "system": system,
            "model": model,
            "hyp_file": path.name,
            "hyp_words": len(normalize(hyp).split()),
            "wer_vs_plaud": vs_plaud["wer"],
            "cer_vs_plaud": vs_plaud["cer"],
            "s_plaud": vs_plaud.get("substitutions"),
            "d_plaud": vs_plaud.get("deletions"),
            "i_plaud": vs_plaud.get("insertions"),
            "h_plaud": vs_plaud.get("hits"),
            "wer_vs_youtube": vs_yt["wer"],
            "cer_vs_youtube": vs_yt["cer"],
            "s_yt": vs_yt.get("substitutions"),
            "d_yt": vs_yt.get("deletions"),
            "i_yt": vs_yt.get("insertions"),
            "h_yt": vs_yt.get("hits"),
            "mean_wer": mean_wer,
            "mean_cer": mean_cer,
            "len_ratio_plaud": len_ratio_plaud,
            "len_ratio_yt": len_ratio_yt,
            "wall_s": wall.get(label),
            "bias_plaud_minus_yt": (
                (vs_plaud["wer"] - vs_yt["wer"])
                if vs_plaud["wer"] is not None and vs_yt["wer"] is not None
                else None
            ),
        }
        rows.append(row)

    rows.sort(key=lambda r: (r["mean_wer"] is None, r["mean_wer"] if r["mean_wer"] is not None else 9e9))

    payload = {
        "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "audio": {
            "source_video": "https://www.youtube.com/watch?v=n0VhIVtviC0",
            "speaker": "Matt Pocock",
            "plaud_file_id": "42e86284875bf900d98cb7e83ce3b7cb",
            "duration_s": 685.08,
        },
        "references": {
            "plaud": {
                "path": str(REF_PLAUD.name),
                "words": len(normalize(plaud).split()),
                "note": "Plaud API transaction (raw ASR segments), speaker labels stripped",
            },
            "youtube": {
                "path": str(REF_YT.name),
                "words": len(normalize(yt).split()),
                "note": "YouTube English auto-generated captions only",
            },
            "cross_wer_youtube_as_hyp_vs_plaud": cross_yt_vs_plaud["wer"],
            "cross_wer_plaud_as_hyp_vs_youtube": cross_plaud_vs_yt["wer"],
        },
        "method": {
            "primary_rank": "mean_wer = average(WER_plaud, WER_youtube)",
            "normalization": "lower, strip punct/timestamps/speaker tags, collapse whitespace, drop um/uh",
            "caveat": "Neither ref is human-verified oracle; dual-ref reduces single-pipeline bias.",
        },
        "results": rows,
    }
    OUT_JSON.write_text(json.dumps(payload, indent=2) + "\n")

    # CSV
    cols = [
        "rank",
        "label",
        "system",
        "model",
        "mean_wer",
        "wer_vs_plaud",
        "wer_vs_youtube",
        "mean_cer",
        "hyp_words",
        "len_ratio_plaud",
        "wall_s",
        "bias_plaud_minus_yt",
    ]
    lines = [",".join(cols)]
    for i, r in enumerate(rows, 1):
        def f(x, pct=False):
            if x is None:
                return ""
            if pct:
                return f"{100*x:.3f}"
            return f"{x:.4f}" if isinstance(x, float) else str(x)

        lines.append(
            ",".join(
                [
                    str(i),
                    r["label"],
                    r["system"],
                    r["model"],
                    f(r["mean_wer"], True),
                    f(r["wer_vs_plaud"], True),
                    f(r["wer_vs_youtube"], True),
                    f(r["mean_cer"], True),
                    str(r["hyp_words"]),
                    f(r["len_ratio_plaud"]),
                    f(r["wall_s"]) if r["wall_s"] is not None else "",
                    f(r["bias_plaud_minus_yt"], True) if r["bias_plaud_minus_yt"] is not None else "",
                ]
            )
        )
    OUT_CSV.write_text("\n".join(lines) + "\n")

    # Markdown report
    def pct(x):
        return f"{100*x:.2f}%" if x is not None else "—"

    md = []
    md += [
        "# Dual-reference STT eval — Matt Pocock lecture",
        "",
        f"- **generated_at_utc:** {payload['generated_at_utc']}",
        f"- **video:** {payload['audio']['source_video']}",
        f"- **plaud file:** `{payload['audio']['plaud_file_id']}` ({payload['audio']['duration_s']:.0f}s)",
        f"- **ref A (Plaud raw):** {payload['references']['plaud']['words']} words",
        f"- **ref B (YouTube auto captions):** {payload['references']['youtube']['words']} words",
        f"- **ref cross-WER (YT vs Plaud):** {pct(cross_yt_vs_plaud['wer'])} "
        f"(refs disagree by ~this much — floor for single-ref claims)",
        "",
        "## Why dual-reference",
        "",
        "The first bench used **only Plaud** as reference. That favors systems that",
        "share Plaud’s ASR/cleanup DNA and treats Plaud errors as model errors.",
        "YouTube auto-captions are an **independent** pipeline on the same speech.",
        "Ranking by **mean WER** across both refs is a better quality proxy without a",
        "human gold transcript.",
        "",
        "## Leaderboard (by mean WER ↑ better)",
        "",
        "| Rank | System | Model | Mean WER | vs Plaud | vs YouTube | Δ(Plaud−YT) | Words | Wall s |",
        "|-----:|--------|-------|---------:|---------:|-----------:|------------:|------:|-------:|",
    ]
    for i, r in enumerate(rows, 1):
        bias = r["bias_plaud_minus_yt"]
        # negative bias => closer to Plaud than YT
        bias_s = pct(bias) if bias is not None else "—"
        wall_s = f"{r['wall_s']:.1f}" if r.get("wall_s") is not None else "—"
        md.append(
            f"| {i} | {r['system']} | `{r['model']}` | {pct(r['mean_wer'])} | "
            f"{pct(r['wer_vs_plaud'])} | {pct(r['wer_vs_youtube'])} | {bias_s} | "
            f"{r['hyp_words']} | {wall_s} |"
        )

    # Best by each ref
    by_plaud = sorted(rows, key=lambda r: r["wer_vs_plaud"] if r["wer_vs_plaud"] is not None else 9)
    by_yt = sorted(rows, key=lambda r: r["wer_vs_youtube"] if r["wer_vs_youtube"] is not None else 9)

    md += [
        "",
        "## Best by reference",
        "",
        f"- **Closest to Plaud:** `{by_plaud[0]['model']}` ({pct(by_plaud[0]['wer_vs_plaud'])})",
        f"- **Closest to YouTube:** `{by_yt[0]['model']}` ({pct(by_yt[0]['wer_vs_youtube'])})",
        f"- **Best mean:** `{rows[0]['model']}` ({pct(rows[0]['mean_wer'])})",
        "",
        "## Aurum local only",
        "",
        "| Model | Mean WER | vs Plaud | vs YouTube | Wall s |",
        "|-------|---------:|---------:|-----------:|-------:|",
    ]
    for r in rows:
        if r["system"] != "aurum-local":
            continue
        wall_s = f"{r['wall_s']:.1f}" if r.get("wall_s") is not None else "—"
        md.append(
            f"| `{r['model']}` | {pct(r['mean_wer'])} | {pct(r['wer_vs_plaud'])} | "
            f"{pct(r['wer_vs_youtube'])} | {wall_s} |"
        )

    md += [
        "",
        "## Error shape (vs YouTube — independent ref)",
        "",
        "| Model | WER | Sub | Del | Ins | Hits |",
        "|-------|----:|----:|----:|----:|-----:|",
    ]
    for r in rows:
        if r["wer_vs_youtube"] is None:
            continue
        md.append(
            f"| `{r['model']}` | {pct(r['wer_vs_youtube'])} | {r['s_yt']} | {r['d_yt']} | "
            f"{r['i_yt']} | {r['h_yt']} |"
        )

    # Bias analysis: models much closer to Plaud than YT
    md += [
        "",
        "## Pipeline affinity (bias)",
        "",
        "`Δ = WER_plaud − WER_youtube`. Negative ⇒ more Plaud-like than YT-like.",
        "",
    ]
    biased = sorted(
        [r for r in rows if r["bias_plaud_minus_yt"] is not None],
        key=lambda r: r["bias_plaud_minus_yt"],
    )
    for r in biased[:5]:
        md.append(
            f"- **Most Plaud-like:** `{r['model']}` Δ={pct(r['bias_plaud_minus_yt'])} "
            f"(P {pct(r['wer_vs_plaud'])} / Y {pct(r['wer_vs_youtube'])})"
        )
        break
    for r in biased[:3]:
        md.append(
            f"  - `{r['model']}` Δ={pct(r['bias_plaud_minus_yt'])}"
        )
    md.append("")
    for r in biased[-3:]:
        md.append(
            f"- **More YT-like / worse on Plaud:** `{r['model']}` Δ={pct(r['bias_plaud_minus_yt'])}"
        )

    # Short hyp detection
    short = [r for r in rows if (r.get("len_ratio_plaud") or 1) < 0.9]
    if short:
        md += [
            "",
            "## Length warnings (possible dropouts / summarization)",
            "",
        ]
        for r in short:
            md.append(
                f"- `{r['model']}`: {r['hyp_words']} words "
                f"(ratio vs Plaud {r['len_ratio_plaud']:.2f})"
            )

    md += [
        "",
        "## Method notes",
        "",
        "1. Hypotheses from prior bench (Aurum local whisper.cpp + OpenRouter transcription models).",
        "2. References: Plaud full raw + YouTube auto captions for the same Matt Pocock talk.",
        "3. Primary score: **mean WER** across both references.",
        "4. S/I/D from `jiwer.process_words` after normalization.",
        "5. Still not a human gold set — for release claims, spot-check hard segments manually.",
        "",
        f"Machine data: `{OUT_JSON.name}`, `{OUT_CSV.name}`.",
        "",
    ]
    OUT_MD.write_text("\n".join(md) + "\n")
    print(f"Wrote {OUT_MD}")
    print(f"Wrote {OUT_JSON}")
    print(f"cross-ref WER YT vs Plaud: {pct(cross_yt_vs_plaud['wer'])}")
    print("Top 5 mean WER:")
    for i, r in enumerate(rows[:5], 1):
        print(
            f"  {i}. {r['label']}: mean={pct(r['mean_wer'])} "
            f"P={pct(r['wer_vs_plaud'])} Y={pct(r['wer_vs_youtube'])}"
        )
    return 0


if __name__ == "__main__":
    # silence unused import if process_words handles
    _ = wer
    raise SystemExit(main())
