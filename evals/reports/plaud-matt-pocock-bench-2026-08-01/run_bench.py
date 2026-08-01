#!/usr/bin/env python3
"""Plaud lecture vs Aurum local + OpenRouter transcription models.

Reference = Plaud cloud transcript (not oracle ground truth).
Metrics: WER/CER (jiwer, normalized), wall seconds, RTF, status.
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

try:
    from jiwer import cer, wer
except ImportError:
    print("jiwer required: pip install jiwer", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent
AURUM = Path(os.environ.get("AURUM_BIN", ROOT.parent.parent / "dist/rc-dogfood/bin/aurum"))
# Prefer mp3 for remote (smaller); 16k wav for local optional
AUDIO = ROOT / "plaud_audio.mp3"
REF_PATH = ROOT / "plaud_ref.txt"
OUT_DIR = ROOT / "hypotheses"
LOG_DIR = ROOT / "logs"
RESULTS = ROOT / "results.jsonl"
REPORT = ROOT / "REPORT.md"

# Representative local catalogue (cached on this machine)
LOCAL_MODELS = [
    "tiny-q5_1",
    "base",
    "base.en",
    "small.en",
    "small",
    "medium.en",
    "large-v3-turbo",
    "large-v3-turbo-q5_0",
    # full large-v3 is slow; still include for quality ceiling
    "large-v3",
]

# https://openrouter.ai/api/v1/models?output_modalities=transcription
OPENROUTER_TRANSCRIPTION = [
    "openai/whisper-1",
    "openai/whisper-large-v3-turbo",
    "openai/whisper-large-v3",
    "openai/gpt-4o-mini-transcribe",
    "openai/gpt-4o-transcribe",
    "x-ai/grok-stt-1.0",
    "deepgram/nova-3",
    "google/chirp-3",
    "microsoft/mai-transcribe-1.5",
    "nvidia/parakeet-tdt-0.6b-v3",
    "mistralai/voxtral-mini-transcribe",
    "qwen/qwen3-asr-flash-2026-02-10",
    "fish-audio/transcribe-1",
]


def normalize(text: str) -> str:
    text = text.lower()
    text = re.sub(r"\[[^\]]*\]", " ", text)  # timestamps
    text = re.sub(r"speaker\s*\d+\s*:\s*", " ", text, flags=re.I)
    text = re.sub(r"[^\w\s']", " ", text)
    text = re.sub(r"\s+", " ", text).strip()
    return text


def metrics(ref: str, hyp: str) -> dict:
    r, h = normalize(ref), normalize(hyp)
    if not r:
        return {"wer": None, "cer": None, "ref_words": 0, "hyp_words": len(h.split())}
    if not h:
        return {
            "wer": 1.0,
            "cer": 1.0,
            "ref_words": len(r.split()),
            "hyp_words": 0,
            "empty_hyp": True,
        }
    return {
        "wer": float(wer(r, h)),
        "cer": float(cer(r, h)),
        "ref_words": len(r.split()),
        "hyp_words": len(h.split()),
    }


def run_cmd(cmd: list[str], timeout: int) -> tuple[int, str, str, float]:
    t0 = time.perf_counter()
    try:
        p = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            env=os.environ.copy(),
        )
        elapsed = time.perf_counter() - t0
        return p.returncode, p.stdout, p.stderr, elapsed
    except subprocess.TimeoutExpired as e:
        elapsed = time.perf_counter() - t0
        out = (e.stdout or b"").decode() if isinstance(e.stdout, (bytes, bytearray)) else (e.stdout or "")
        err = (e.stderr or b"").decode() if isinstance(e.stderr, (bytes, bytearray)) else (e.stderr or "")
        return 124, out, err + "\nTIMEOUT", elapsed


def sanitize(s: str) -> str:
    return re.sub(r"sk-or-v1-[A-Za-z0-9]+", "[REDACTED_KEY]", s or "")


def run_one(
    *,
    label: str,
    provider: str,
    model: str,
    audio: Path,
    mode: str | None,
    timeout: int,
    ref: str,
    audio_secs: float,
) -> dict:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    safe = re.sub(r"[^\w.-]+", "_", label)
    hyp_path = OUT_DIR / f"{safe}.txt"
    log_path = LOG_DIR / f"{safe}.log"

    cmd = [
        str(AURUM),
        "transcribe",
        str(audio),
        "--provider",
        provider,
        "--model",
        model,
        "-o",
        "txt",
        "--output-file",
        str(hyp_path),
    ]
    if mode:
        cmd.extend(["--openrouter-stt-mode", mode])

    print(f"\n== {label} ==", flush=True)
    print(" ", " ".join(cmd[:6]), "...", flush=True)
    rc, stdout, stderr, elapsed = run_cmd(cmd, timeout=timeout)
    log_path.write_text(sanitize(stdout + "\n--- stderr ---\n" + stderr))

    hyp = hyp_path.read_text() if hyp_path.exists() else ""
    # strip any accidental empty file
    ok = rc == 0 and bool(hyp.strip())
    m = metrics(ref, hyp) if ok else metrics(ref, hyp)
    row = {
        "label": label,
        "provider": provider,
        "model": model,
        "openrouter_stt_mode": mode,
        "status": "pass" if ok else "fail",
        "rc": rc,
        "wall_s": round(elapsed, 2),
        "rtf": round(elapsed / audio_secs, 4) if audio_secs > 0 else None,
        "hyp_chars": len(hyp),
        "hyp_path": str(hyp_path.relative_to(ROOT)),
        **m,
        "error_snippet": sanitize(stderr.strip().splitlines()[-1])[:240] if not ok else None,
        "ts_utc": datetime.now(timezone.utc).isoformat(),
    }
    with RESULTS.open("a") as f:
        f.write(json.dumps(row) + "\n")
    wer_s = f"{row['wer']*100:.1f}%" if row.get("wer") is not None and ok else "n/a"
    print(
        f"  status={row['status']} wall={row['wall_s']}s rtf={row['rtf']} wer={wer_s}",
        flush=True,
    )
    if not ok:
        print(f"  err={row['error_snippet']}", flush=True)
    return row


def write_report(rows: list[dict], meta: dict) -> None:
    def sort_key(r):
        if r.get("status") != "pass" or r.get("wer") is None:
            return (1, 9e9, r["label"])
        return (0, r["wer"], r["label"])

    rows_sorted = sorted(rows, key=sort_key)
    lines = [
        "# Plaud vs Aurum / OpenRouter transcription bench",
        "",
        f"- **generated_at_utc:** {meta['generated_at_utc']}",
        f"- **audio:** {meta['audio_name']} ({meta['audio_secs']:.1f}s, {meta['audio_bytes']} bytes)",
        f"- **reference:** Plaud transcript for `{meta['file_id']}` — "
        f"\"{meta['title']}\" (normalized plain text, speaker labels/timestamps stripped)",
        f"- **ref_words:** {meta['ref_words']}",
        f"- **aurum_bin:** `{meta['aurum_bin']}` ({meta['aurum_version']})",
        f"- **note:** WER/CER are vs **Plaud transcript as reference**, not human-verified ground truth. "
        "Plaud may already use a commercial ASR + cleanup; low WER means agreement with Plaud, not absolute accuracy.",
        "",
        "## Summary table (best WER first)",
        "",
        "| Rank | Label | Provider | Model | Status | WER | CER | Wall s | RTF |",
        "|-----:|-------|----------|-------|--------|----:|----:|-------:|----:|",
    ]
    rank = 0
    for r in rows_sorted:
        rank += 1
        wer_s = f"{r['wer']*100:.2f}%" if r.get("wer") is not None and r["status"] == "pass" else "—"
        cer_s = f"{r['cer']*100:.2f}%" if r.get("cer") is not None and r["status"] == "pass" else "—"
        lines.append(
            f"| {rank} | {r['label']} | {r['provider']} | `{r['model']}` | {r['status']} | "
            f"{wer_s} | {cer_s} | {r['wall_s']} | {r.get('rtf') if r.get('rtf') is not None else '—'} |"
        )

    lines += [
        "",
        "## Local Aurum (whisper.cpp)",
        "",
    ]
    for r in rows_sorted:
        if r["provider"] != "local":
            continue
        lines.append(
            f"- **{r['model']}**: status={r['status']} "
            f"WER={r['wer']*100:.2f}% wall={r['wall_s']}s"
            if r.get("wer") is not None and r["status"] == "pass"
            else f"- **{r['model']}**: status={r['status']} err={r.get('error_snippet')}"
        )

    lines += ["", "## OpenRouter `output_modalities=transcription`", ""]
    for r in rows_sorted:
        if r["provider"] != "openrouter":
            continue
        if r.get("wer") is not None and r["status"] == "pass":
            lines.append(
                f"- **{r['model']}**: WER={r['wer']*100:.2f}% wall={r['wall_s']}s "
                f"mode={r.get('openrouter_stt_mode')}"
            )
        else:
            lines.append(
                f"- **{r['model']}**: FAIL — {r.get('error_snippet')}"
            )

    lines += [
        "",
        "## Method",
        "",
        "1. Download Plaud audio + transcript via authenticated `plaud` CLI.",
        "2. Normalize reference (strip `[timestamps]` and `Speaker N:`).",
        "3. Transcribe with Aurum CLI (`provider=local|openrouter`).",
        "4. OpenRouter dedicated ASR path: `--openrouter-stt-mode transcriptions`.",
        "5. Metrics via `jiwer` after case-fold + punctuation strip.",
        "",
        f"Raw rows: `{RESULTS.name}`. Hypotheses under `hypotheses/`.",
        "",
    ]
    REPORT.write_text("\n".join(lines) + "\n")
    print(f"\nWrote {REPORT}", flush=True)


def main() -> int:
    if not AURUM.is_file():
        print(f"missing aurum binary: {AURUM}", file=sys.stderr)
        return 1
    if not AUDIO.is_file() or not REF_PATH.is_file():
        print("missing audio or plaud_ref.txt", file=sys.stderr)
        return 1

    ver = subprocess.check_output([str(AURUM), "--version"], text=True).strip()
    ref = REF_PATH.read_text()
    audio_secs = 685.08  # from ffprobe
    # allow override via ffprobe
    try:
        import json as _json

        p = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "json",
                str(AUDIO),
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        audio_secs = float(_json.loads(p.stdout)["format"]["duration"])
    except Exception:
        pass

    # fresh results for this full run
    if RESULTS.exists():
        RESULTS.rename(RESULTS.with_suffix(".jsonl.bak"))
    OUT_DIR.mkdir(exist_ok=True)
    LOG_DIR.mkdir(exist_ok=True)

    only = os.environ.get("BENCH_ONLY", "").strip()  # local | openrouter | all
    rows: list[dict] = []

    if only in ("", "all", "local"):
        for model in LOCAL_MODELS:
            # large models get long timeout
            timeout = 3600 if "large" in model or "medium" in model else 1800
            rows.append(
                run_one(
                    label=f"local/{model}",
                    provider="local",
                    model=model,
                    audio=AUDIO,
                    mode=None,
                    timeout=timeout,
                    ref=ref,
                    audio_secs=audio_secs,
                )
            )

    if only in ("", "all", "openrouter"):
        if not os.environ.get("OPENROUTER_API_KEY"):
            print("OPENROUTER_API_KEY unset — skipping openrouter suite", file=sys.stderr)
        else:
            for model in OPENROUTER_TRANSCRIPTION:
                rows.append(
                    run_one(
                        label=f"openrouter/{model.replace('/', '_')}",
                        provider="openrouter",
                        model=model,
                        audio=AUDIO,
                        mode="transcriptions",
                        timeout=1800,
                        ref=ref,
                        audio_secs=audio_secs,
                    )
                )

    meta = {
        "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "audio_name": AUDIO.name,
        "audio_secs": audio_secs,
        "audio_bytes": AUDIO.stat().st_size,
        "file_id": "42e86284875bf900d98cb7e83ce3b7cb",
        "title": "07-30 Lecture: AI-Driven Code Generation and Prototyping",
        "ref_words": len(normalize(ref).split()),
        "aurum_bin": str(AURUM),
        "aurum_version": ver,
    }
    (ROOT / "meta.json").write_text(json.dumps(meta, indent=2) + "\n")
    write_report(rows, meta)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
