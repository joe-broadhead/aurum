#!/usr/bin/env python3
"""Build a hard-segment gold subset for human spot-check.

Uses timed Plaud segments + YouTube auto captions as a timed spine, ranks
windows by ref disagreement and multi-hypothesis lexical entropy, cuts audio
clips with ffmpeg, and writes an annotation template (gold left blank).
"""
from __future__ import annotations

import json
import re
import subprocess
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

from jiwer import wer

ROOT = Path(__file__).resolve().parent
AUDIO = ROOT / "plaud_audio.mp3"
PLAUD_SEGS = ROOT / "plaud_source_transaction.json"
YT_JSON = ROOT / "youtube_transcript_raw.json"
HYP_DIR = ROOT / "hypotheses"
OUT = ROOT / "gold_subset"
N_HARD = 8
N_EASY = 2  # control clips where refs agree


def norm(t: str) -> str:
    t = t.lower()
    t = re.sub(r"[^\w\s']", " ", t)
    t = re.sub(r"\s+", " ", t).strip()
    return t


def fmt_ts(sec: float) -> str:
    sec = max(0.0, float(sec))
    m, s = divmod(int(sec), 60)
    h, m = divmod(m, 60)
    if h:
        return f"{h:02d}:{m:02d}:{s:02d}"
    return f"{m:02d}:{s:02d}"


def plaud_times_ms_to_s(segs: list[dict]) -> list[dict]:
    out = []
    for s in segs:
        out.append(
            {
                "start": float(s["start_time"]) / 1000.0,
                "end": float(s["end_time"]) / 1000.0,
                "text": (s.get("content") or "").strip(),
                "speaker": s.get("speaker"),
            }
        )
    return out


def yt_windows(snippets: list[dict], win_s: float = 45.0, hop_s: float = 30.0) -> list[dict]:
    if not snippets:
        return []
    duration = max((s["start"] + s.get("duration", 0) for s in snippets), default=0)
    windows = []
    t = 0.0
    while t < duration:
        end = min(t + win_s, duration)
        parts = []
        for s in snippets:
            ss, dd = s["start"], s.get("duration") or 0
            se = ss + dd
            if se <= t or ss >= end:
                continue
            parts.append((s.get("text") or "").replace("\n", " ").strip())
        text = re.sub(r"\s+", " ", " ".join(parts)).strip()
        if len(text.split()) >= 12:
            windows.append({"start": t, "end": end, "text": text, "source": "youtube"})
        t += hop_s
    return windows


def plaud_as_windows(segs: list[dict]) -> list[dict]:
    return [
        {
            "start": s["start"],
            "end": s["end"],
            "text": s["text"],
            "source": "plaud",
        }
        for s in segs
        if len(s["text"].split()) >= 12
    ]


def yt_text_in_range(snippets: list[dict], start: float, end: float) -> str:
    parts = []
    for s in snippets:
        ss, dd = s["start"], s.get("duration") or 0
        se = ss + dd
        if se <= start or ss >= end:
            continue
        parts.append((s.get("text") or "").replace("\n", " ").strip())
    return re.sub(r"\s+", " ", " ".join(parts)).strip()


def plaud_text_in_range(segs: list[dict], start: float, end: float) -> str:
    # weighted overlap of plaud mega-segments
    parts = []
    for s in segs:
        if s["end"] <= start or s["start"] >= end:
            continue
        parts.append(s["text"])
    return re.sub(r"\s+", " ", " ".join(parts)).strip()


def extract_hyp_span(hyp_full: str, anchor: str) -> str:
    """Best-effort: find hyp subsequence most similar to anchor via window search."""
    a = norm(anchor).split()
    h = norm(hyp_full).split()
    if not a or not h:
        return ""
    n, m = len(a), len(h)
    # search windows of length ~n ± 20%
    lo = max(1, int(n * 0.7))
    hi = min(m, int(n * 1.3) + 1)
    best = ""
    best_score = 1e9
    # stride for speed
    stride = max(1, n // 8)
    for wlen in range(lo, hi + 1, max(1, (hi - lo) // 6 or 1)):
        for i in range(0, m - wlen + 1, stride):
            cand = h[i : i + wlen]
            # cheap score: token set jaccard inverted + length penalty
            sa, sc = set(a), set(cand)
            if not sa or not sc:
                continue
            j = 1 - len(sa & sc) / len(sa | sc)
            score = j + 0.05 * abs(len(cand) - n) / n
            if score < best_score:
                best_score = score
                best = " ".join(cand)
    return best


def multi_hyp_disagreement(anchor: str, hyps: dict[str, str]) -> float:
    """Mean pairwise WER among hyp spans extracted around anchor (0=agree)."""
    spans = []
    for name, full in hyps.items():
        sp = extract_hyp_span(full, anchor)
        if sp:
            spans.append(sp)
    if len(spans) < 2:
        return 0.0
    scores = []
    for i in range(len(spans)):
        for j in range(i + 1, len(spans)):
            try:
                scores.append(float(wer(spans[i], spans[j])))
            except Exception:
                pass
    return sum(scores) / len(scores) if scores else 0.0


def cut_audio(start: float, end: float, dest: Path) -> None:
    # pad slightly for context
    pad = 0.4
    ss = max(0.0, start - pad)
    dur = (end - start) + 2 * pad
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-ss",
            f"{ss:.3f}",
            "-i",
            str(AUDIO),
            "-t",
            f"{dur:.3f}",
            "-ac",
            "1",
            "-ar",
            "16000",
            str(dest),
        ],
        check=True,
        capture_output=True,
    )


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    clips_dir = OUT / "clips"
    clips_dir.mkdir(exist_ok=True)

    plaud_segs = plaud_times_ms_to_s(json.loads(PLAUD_SEGS.read_text()))
    yt_snips = json.loads(YT_JSON.read_text())

    # candidate windows: youtube sliding + plaud native segments
    cands = yt_windows(yt_snips, win_s=50.0, hop_s=35.0)
    # also add plaud segments as candidates
    for w in plaud_as_windows(plaud_segs):
        cands.append(w)

    # load a subset of hyps for disagreement (top locals + a few remote)
    hyp_files = {
        "medium.en": HYP_DIR / "local_medium.en.txt",
        "large-v3-turbo": HYP_DIR / "local_large-v3-turbo.txt",
        "small.en": HYP_DIR / "local_small.en.txt",
        "fish-audio": HYP_DIR / "or_direct_fish-audio_transcribe-1.txt",
        "mai": HYP_DIR / "or_direct_microsoft_mai-transcribe-1.5.txt",
        "voxtral": HYP_DIR / "or_direct_mistralai_voxtral-mini-transcribe.txt",
        "whisper-1": HYP_DIR / "or_direct_openai_whisper-1.txt",
        "grok-stt": HYP_DIR / "or_direct_x-ai_grok-stt-1.0.txt",
    }
    hyps = {k: p.read_text() for k, p in hyp_files.items() if p.exists()}

    scored = []
    for w in cands:
        start, end = w["start"], w["end"]
        # unify texts for this window
        ptxt = plaud_text_in_range(plaud_segs, start, end)
        ytxt = yt_text_in_range(yt_snips, start, end)
        if len(norm(ptxt).split()) < 10 or len(norm(ytxt).split()) < 10:
            continue
        try:
            ref_wer = float(wer(norm(ptxt), norm(ytxt)))
        except Exception:
            ref_wer = 0.0
        # multi-hyp disagreement vs youtube anchor (independent)
        mhd = multi_hyp_disagreement(ytxt, hyps)
        # hardness score: emphasize ref disagreement + model disagreement
        hardness = 0.55 * ref_wer + 0.45 * mhd
        scored.append(
            {
                "start": start,
                "end": end,
                "duration": end - start,
                "plaud_text": ptxt,
                "youtube_text": ytxt,
                "ref_cross_wer": ref_wer,
                "multi_hyp_disagreement": mhd,
                "hardness": hardness,
                "source": w.get("source"),
            }
        )

    # dedupe overlapping windows: keep hardest non-overlapping
    scored.sort(key=lambda x: -x["hardness"])
    selected_hard: list[dict] = []
    for c in scored:
        if any(not (c["end"] <= s["start"] or c["start"] >= s["end"]) for s in selected_hard):
            continue
        selected_hard.append(c)
        if len(selected_hard) >= N_HARD:
            break

    # easy controls: lowest hardness, non-overlapping with hard
    scored_easy = sorted(scored, key=lambda x: x["hardness"])
    selected_easy: list[dict] = []
    for c in scored_easy:
        if any(
            not (c["end"] <= s["start"] or c["start"] >= s["end"])
            for s in selected_hard + selected_easy
        ):
            continue
        if c["hardness"] > 0.08:  # still "easy"
            continue
        selected_easy.append(c)
        if len(selected_easy) >= N_EASY:
            break

    items = []
    for kind, group in (("hard", selected_hard), ("easy", selected_easy)):
        for c in group:
            c = dict(c)
            c["kind"] = kind
            items.append(c)
    items.sort(key=lambda x: x["start"])

    # cut clips + write per-clip package
    manifest = {
        "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "video": "https://www.youtube.com/watch?v=n0VhIVtviC0",
        "speaker": "Matt Pocock",
        "plaud_file_id": "42e86284875bf900d98cb7e83ce3b7cb",
        "instructions": (
            "Listen to each clip. Write verbatim gold in gold.txt "
            "(what was actually said). Prefer orthography of clear speech; "
            "do not copy Plaud or YouTube blindly when they disagree."
        ),
        "clips": [],
    }

    md = [
        "# Gold subset — human annotation template",
        "",
        f"- **generated:** {manifest['generated_at_utc']}",
        f"- **source video:** {manifest['video']}",
        f"- **clips:** {len(items)} ({N_HARD} hard + {N_EASY} easy controls)",
        "",
        "## How to annotate",
        "",
        "1. Open `clips/clip_XX/audio.wav` (or play while reading).",
        "2. Read `plaud_draft.txt` and `youtube_draft.txt` as **hints only**.",
        "3. Write what you hear into `gold.txt` (one paragraph, no timestamps).",
        "4. Optional: note uncertainty in `notes.txt`.",
        "5. When all gold.txt filled, run `python3 score_against_gold.py`.",
        "",
        "## Clip index",
        "",
        "| ID | Kind | Time | Hardness | Cross-WER | Multi-hyp |",
        "|----|------|------|---------:|----------:|----------:|",
    ]

    for i, c in enumerate(items, 1):
        cid = f"clip_{i:02d}"
        cdir = clips_dir / cid
        cdir.mkdir(exist_ok=True)
        audio_path = cdir / "audio.wav"
        print(f"cutting {cid} {fmt_ts(c['start'])}-{fmt_ts(c['end'])} hardness={c['hardness']:.3f}")
        cut_audio(c["start"], c["end"], audio_path)
        (cdir / "plaud_draft.txt").write_text(c["plaud_text"].strip() + "\n")
        (cdir / "youtube_draft.txt").write_text(c["youtube_text"].strip() + "\n")
        (cdir / "gold.txt").write_text("")  # human fills
        (cdir / "notes.txt").write_text("")
        meta = {
            "id": cid,
            "kind": c["kind"],
            "start_s": round(c["start"], 3),
            "end_s": round(c["end"], 3),
            "time": f"{fmt_ts(c['start'])}-{fmt_ts(c['end'])}",
            "hardness": round(c["hardness"], 4),
            "ref_cross_wer": round(c["ref_cross_wer"], 4),
            "multi_hyp_disagreement": round(c["multi_hyp_disagreement"], 4),
            "gold_status": "empty",
        }
        (cdir / "meta.json").write_text(json.dumps(meta, indent=2) + "\n")
        manifest["clips"].append(meta)
        md.append(
            f"| {cid} | {c['kind']} | {meta['time']} | {meta['hardness']:.3f} | "
            f"{100*meta['ref_cross_wer']:.1f}% | {100*meta['multi_hyp_disagreement']:.1f}% |"
        )
        # also dump hyp spans for annotator interest
        spans = {}
        for name, full in hyps.items():
            spans[name] = extract_hyp_span(full, c["youtube_text"])
        (cdir / "model_spans.json").write_text(json.dumps(spans, indent=2) + "\n")

    (OUT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (OUT / "README.md").write_text("\n".join(md) + "\n")

    # scorer stub that works once gold filled
    scorer = r'''#!/usr/bin/env python3
"""Score existing full-file hypotheses against filled gold clips (concat or per-clip)."""
from __future__ import annotations
import json, re
from pathlib import Path
from jiwer import wer, cer

ROOT = Path(__file__).resolve().parent
HYP_DIR = ROOT.parent / "hypotheses"

def norm(t: str) -> str:
    t = t.lower()
    t = re.sub(r"[^\w\s']", " ", t)
    return re.sub(r"\s+", " ", t).strip()

def main():
    clips = sorted((ROOT / "clips").glob("clip_*"))
    gold_parts = []
    missing = []
    for c in clips:
        g = (c / "gold.txt").read_text().strip()
        if not g:
            missing.append(c.name)
        else:
            gold_parts.append(g)
    if missing:
        print("Gold not filled for:", ", ".join(missing))
        print("Fill gold.txt in each clip directory first.")
        return 1
    gold = norm(" ".join(gold_parts))
    print(f"gold words={len(gold.split())} from {len(gold_parts)} clips\n")
    rows = []
    for p in sorted(HYP_DIR.glob("*.txt")):
        hyp = norm(p.read_text())
        # full-file hyp vs gold subset is not ideal; report per-clip instead
        rows.append(p.name)
    # per-clip scoring using model_spans if present, else skip full-file
    print("Per-clip WER vs gold (using model_spans.json when present):\n")
    # rebuild spans against gold text
    hyp_files = list(HYP_DIR.glob("*.txt"))
    summary = []
    for p in hyp_files:
        full = p.read_text()
        wers = []
        for c in clips:
            g = norm((c / "gold.txt").read_text())
            # extract span from full hyp near gold
            # simple: use youtube draft as locator then score span vs gold
            anchor = (c / "youtube_draft.txt").read_text()
            # reuse crude extract
            from build_gold_subset import extract_hyp_span, norm as n2
            span = extract_hyp_span(full, anchor) or extract_hyp_span(full, g)
            if not span:
                continue
            wers.append(float(wer(g, norm(span))))
        if wers:
            m = sum(wers)/len(wers)
            summary.append((m, p.name, len(wers)))
    summary.sort()
    print(f"{'mean_wer':>10}  clips  file")
    for m, name, n in summary:
        print(f"{100*m:9.2f}%  {n:5d}  {name}")
    Path(ROOT / "gold_scores.json").write_text(json.dumps([
        {"file": n, "mean_wer": m, "n_clips": k} for m,n,k in summary
    ], indent=2))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
'''
    (OUT / "score_against_gold.py").write_text(scorer)
    print(f"\nWrote {len(items)} clips → {OUT}")
    print(f"Hard: {len(selected_hard)} Easy: {len(selected_easy)}")


if __name__ == "__main__":
    main()
