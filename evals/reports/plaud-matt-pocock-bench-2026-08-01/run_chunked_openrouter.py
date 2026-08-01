#!/usr/bin/env python3
"""Re-bench OpenRouter models via Aurum CLI using audio chunking.

Aurum rejects single-segment remote transcripts > max_segment_chars (8000).
We split the lecture into ~N-second chunks, call:
  aurum transcribe CHUNK --provider openrouter --model M --openrouter-stt-mode transcriptions
then stitch hypotheses and dual-score vs Plaud + YouTube.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

from jiwer import cer, process_words, wer

ROOT = Path(__file__).resolve().parent
AUDIO = ROOT / "plaud_audio.mp3"
AURUM = Path(os.environ.get("AURUM_BIN", ROOT.parent / "rc-dogfood/bin/aurum"))
REF_PLAUD = ROOT / "plaud_transcript_full_raw.txt"
REF_YT = ROOT / "youtube_transcript_plain.txt"
CHUNK_DIR = ROOT / "chunks"
HYP_DIR = ROOT / "hypotheses"
OUT_JSONL = ROOT / "results_openrouter_chunked.jsonl"
OUT_MD = ROOT / "CHUNKED_OPENROUTER_REPORT.md"

# ~3.5 min chunks → each transcript typically well under 8k chars
CHUNK_SEC = float(os.environ.get("CHUNK_SEC", "210"))
OVERLAP_SEC = float(os.environ.get("CHUNK_OVERLAP", "0"))

MODELS = [
    "openai/whisper-1",
    "openai/whisper-large-v3-turbo",
    "openai/whisper-large-v3",
    "openai/gpt-4o-mini-transcribe",
    "openai/gpt-4o-transcribe",
    "x-ai/grok-stt-1.0",
    "deepgram/nova-3",
    "microsoft/mai-transcribe-1.5",
    "nvidia/parakeet-tdt-0.6b-v3",
    "mistralai/voxtral-mini-transcribe",
    "fish-audio/transcribe-1",
    # google/chirp + qwen failed direct before; retry via aurum chunked
    "google/chirp-3",
    "qwen/qwen3-asr-flash-2026-02-10",
]


def normalize(text: str) -> str:
    text = text.lower()
    text = re.sub(r"\[[^\]]*\]", " ", text)
    text = re.sub(r"speaker\s*\d+\s*:\s*", " ", text, flags=re.I)
    text = text.replace("’", "'")
    text = re.sub(r"\b(um|uh|erm)\b", " ", text)
    text = re.sub(r"[^\w\s']", " ", text)
    return re.sub(r"\s+", " ", text).strip()


def score(ref: str, hyp: str) -> dict:
    r, h = normalize(ref), normalize(hyp)
    if not r or not h:
        return {"wer": 1.0 if r and not h else None, "cer": 1.0 if r and not h else None}
    out = process_words(r, h)
    return {
        "wer": float(out.wer),
        "cer": float(cer(r, h)),
        "substitutions": int(out.substitutions),
        "deletions": int(out.deletions),
        "insertions": int(out.insertions),
        "hits": int(out.hits),
        "hyp_words": len(h.split()),
        "ref_words": len(r.split()),
    }


def sanitize(s: str) -> str:
    return re.sub(r"sk-or-v1-[A-Za-z0-9]+", "[REDACTED]", s or "")


def audio_duration(path: Path) -> float:
    p = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            str(path),
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    return float(p.stdout.strip())


def make_chunks(duration: float) -> list[Path]:
    CHUNK_DIR.mkdir(parents=True, exist_ok=True)
    paths: list[Path] = []
    start = 0.0
    idx = 0
    while start < duration - 0.5:
        end = min(start + CHUNK_SEC, duration)
        out = CHUNK_DIR / f"chunk_{idx:02d}_{int(start)}-{int(end)}.mp3"
        if not out.exists() or out.stat().st_size < 1000:
            subprocess.run(
                [
                    "ffmpeg",
                    "-y",
                    "-ss",
                    f"{start:.3f}",
                    "-i",
                    str(AUDIO),
                    "-t",
                    f"{end - start:.3f}",
                    "-c",
                    "copy",
                    str(out),
                ],
                check=True,
                capture_output=True,
            )
        paths.append(out)
        idx += 1
        start = end - OVERLAP_SEC
        if end >= duration:
            break
    return paths


def aurum_transcribe(chunk: Path, model: str, out_txt: Path, timeout: int = 600) -> tuple[int, float, str]:
    out_txt.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        str(AURUM),
        "transcribe",
        str(chunk),
        "--provider",
        "openrouter",
        "--model",
        model,
        "--openrouter-stt-mode",
        "transcriptions",
        "-o",
        "txt",
        "--output-file",
        str(out_txt),
    ]
    t0 = time.perf_counter()
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=os.environ.copy())
    elapsed = time.perf_counter() - t0
    err = sanitize(p.stderr.strip() or p.stdout.strip())
    return p.returncode, elapsed, err


def run_model(model: str, chunks: list[Path], audio_secs: float) -> dict:
    safe = re.sub(r"[^\w.-]+", "_", model)
    work = ROOT / "chunked_work" / safe
    work.mkdir(parents=True, exist_ok=True)
    parts: list[str] = []
    total_wall = 0.0
    errors: list[str] = []
    print(f"\n== aurum-chunked/{model} ({len(chunks)} chunks) ==", flush=True)
    for i, ch in enumerate(chunks):
        out_txt = work / f"part_{i:02d}.txt"
        try:
            rc, wall, err = aurum_transcribe(ch, model, out_txt)
        except subprocess.TimeoutExpired:
            rc, wall, err = 124, float(timeout := 600), "TIMEOUT"
        total_wall += wall
        text = out_txt.read_text().strip() if out_txt.exists() else ""
        if rc != 0 or not text:
            errors.append(f"chunk{i}:rc={rc}:{err[:160]}")
            print(f"  chunk {i}: FAIL {err[:120]}", flush=True)
            # continue to try remaining chunks
            continue
        parts.append(text)
        print(f"  chunk {i}: ok wall={wall:.1f}s chars={len(text)}", flush=True)

    stitched = re.sub(r"\s+", " ", " ".join(parts)).strip()
    hyp_path = HYP_DIR / f"aurum_chunked_{safe}.txt"
    hyp_path.write_text(stitched + ("\n" if stitched else ""))
    ok = bool(stitched) and not errors
    # partial success still scorable if we got most chunks
    status = "pass" if stitched and len(parts) == len(chunks) else ("partial" if stitched else "fail")

    plaud = REF_PLAUD.read_text()
    yt = REF_YT.read_text()
    sp = score(plaud, stitched) if stitched else {}
    sy = score(yt, stitched) if stitched else {}
    mean_wer = None
    if sp.get("wer") is not None and sy.get("wer") is not None:
        mean_wer = (sp["wer"] + sy["wer"]) / 2.0

    row = {
        "label": f"aurum-chunked/{model}",
        "provider": "aurum-openrouter-chunked",
        "model": model,
        "status": status,
        "chunks_ok": len(parts),
        "chunks_total": len(chunks),
        "wall_s": round(total_wall, 2),
        "rtf": round(total_wall / audio_secs, 4) if audio_secs else None,
        "hyp_path": str(hyp_path.relative_to(ROOT)),
        "hyp_words": len(normalize(stitched).split()) if stitched else 0,
        "wer_vs_plaud": sp.get("wer"),
        "wer_vs_youtube": sy.get("wer"),
        "mean_wer": mean_wer,
        "cer_vs_plaud": sp.get("cer"),
        "cer_vs_youtube": sy.get("cer"),
        "errors": errors[:5],
        "ts_utc": datetime.now(timezone.utc).isoformat(),
    }
    wer_s = f"{100*mean_wer:.2f}%" if mean_wer is not None else "n/a"
    print(f"  → {status} mean_wer={wer_s} wall={total_wall:.1f}s words={row['hyp_words']}", flush=True)
    return row


def write_report(rows: list[dict], meta: dict) -> None:
    def pct(x):
        return f"{100*x:.2f}%" if x is not None else "—"

    rows_sorted = sorted(
        rows,
        key=lambda r: (r.get("mean_wer") is None, r.get("mean_wer") if r.get("mean_wer") is not None else 9),
    )
    lines = [
        "# Aurum OpenRouter chunked re-bench",
        "",
        f"- **generated_at_utc:** {meta['generated_at_utc']}",
        f"- **aurum:** {meta['aurum_version']}",
        f"- **chunk_sec:** {meta['chunk_sec']}",
        f"- **chunks:** {meta['n_chunks']}",
        f"- **audio:** {meta['audio_secs']:.1f}s Matt Pocock lecture",
        f"- **path:** `aurum transcribe --provider openrouter --openrouter-stt-mode transcriptions` per chunk, stitch",
        "",
        "This exercises the **real Aurum CLI remote path** (not direct OpenRouter API),",
        "working around `max_segment_chars=8000` via client-side chunking.",
        "",
        "## Leaderboard (mean WER vs Plaud + YouTube)",
        "",
        "| Rank | Model | Status | Mean WER | vs Plaud | vs YouTube | Chunks | Wall s |",
        "|-----:|-------|--------|---------:|---------:|-----------:|-------:|-------:|",
    ]
    for i, r in enumerate(rows_sorted, 1):
        lines.append(
            f"| {i} | `{r['model']}` | {r['status']} | {pct(r.get('mean_wer'))} | "
            f"{pct(r.get('wer_vs_plaud'))} | {pct(r.get('wer_vs_youtube'))} | "
            f"{r['chunks_ok']}/{r['chunks_total']} | {r['wall_s']} |"
        )
    lines += [
        "",
        "## Failures / partials",
        "",
    ]
    for r in rows_sorted:
        if r["status"] == "pass":
            continue
        err = "; ".join(r.get("errors") or []) or "n/a"
        lines.append(f"- `{r['model']}` ({r['status']}): {err[:200]}")
    lines += [
        "",
        "## Notes",
        "",
        "- Dual-ref mean WER matches the better-eval methodology.",
        "- Compare to `EVAL_REPORT.md` (direct OpenRouter API hyps) to see CLI path parity.",
        "- Product fix options: raise `max_segment_chars`, or document chunking for long audio.",
        "",
        f"Rows: `{OUT_JSONL.name}`. Hyps: `hypotheses/aurum_chunked_*.txt`.",
        "",
    ]
    OUT_MD.write_text("\n".join(lines) + "\n")
    print(f"\nWrote {OUT_MD}", flush=True)


def main() -> int:
    if not AURUM.is_file():
        print("missing aurum", AURUM, file=sys.stderr)
        return 1
    if not os.environ.get("OPENROUTER_API_KEY"):
        print("OPENROUTER_API_KEY required", file=sys.stderr)
        return 1
    ver = subprocess.check_output([str(AURUM), "--version"], text=True).strip()
    dur = audio_duration(AUDIO)
    print(f"audio duration={dur:.1f}s chunk_sec={CHUNK_SEC}", flush=True)
    chunks = make_chunks(dur)
    print(f"chunks={len(chunks)}", flush=True)

    only = os.environ.get("OR_ONLY", "").strip()
    models = [only] if only else MODELS

    if OUT_JSONL.exists():
        OUT_JSONL.rename(OUT_JSONL.with_suffix(".jsonl.bak"))

    rows = []
    for model in models:
        row = run_model(model, chunks, dur)
        rows.append(row)
        with OUT_JSONL.open("a") as f:
            f.write(json.dumps(row) + "\n")

    meta = {
        "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "aurum_version": ver,
        "chunk_sec": CHUNK_SEC,
        "n_chunks": len(chunks),
        "audio_secs": dur,
    }
    write_report(rows, meta)
    (ROOT / "chunked_meta.json").write_text(json.dumps(meta, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    _ = wer
    raise SystemExit(main())
