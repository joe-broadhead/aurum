#!/usr/bin/env python3
"""Direct OpenRouter /audio/transcriptions bench (bypass Aurum single-segment 8k cap).

Still scores vs Plaud reference. Appends to results_openrouter.jsonl and rewrites REPORT.md
including local rows from results.jsonl.
"""
from __future__ import annotations

import json
import os
import re
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import requests
from jiwer import cer, wer

ROOT = Path(__file__).resolve().parent
AUDIO = ROOT / "plaud_audio.mp3"
REF_PATH = ROOT / "plaud_ref.txt"
HYP = ROOT / "hypotheses"
LOG = ROOT / "logs"
RESULTS_OR = ROOT / "results_openrouter.jsonl"
RESULTS_LOCAL = ROOT / "results.jsonl"
REPORT = ROOT / "REPORT.md"
API = os.environ.get("OPENROUTER_BASE", "https://openrouter.ai/api/v1")

MODELS = [
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
    text = re.sub(r"\[[^\]]*\]", " ", text)
    text = re.sub(r"speaker\s*\d+\s*:\s*", " ", text, flags=re.I)
    text = re.sub(r"[^\w\s']", " ", text)
    text = re.sub(r"\s+", " ", text).strip()
    return text


def metrics(ref: str, hyp: str) -> dict:
    r, h = normalize(ref), normalize(hyp)
    if not r:
        return {"wer": None, "cer": None, "ref_words": 0, "hyp_words": len(h.split())}
    if not h:
        return {"wer": 1.0, "cer": 1.0, "ref_words": len(r.split()), "hyp_words": 0}
    return {
        "wer": float(wer(r, h)),
        "cer": float(cer(r, h)),
        "ref_words": len(r.split()),
        "hyp_words": len(h.split()),
    }


def extract_text(payload) -> str:
    if payload is None:
        return ""
    if isinstance(payload, str):
        # sometimes plain text
        try:
            payload = json.loads(payload)
        except json.JSONDecodeError:
            return payload
    if isinstance(payload, dict):
        if isinstance(payload.get("text"), str):
            return payload["text"]
        # nested
        for k in ("transcript", "result", "output"):
            v = payload.get(k)
            if isinstance(v, str):
                return v
            if isinstance(v, dict) and isinstance(v.get("text"), str):
                return v["text"]
        # choices chat-like
        choices = payload.get("choices")
        if isinstance(choices, list) and choices:
            msg = choices[0].get("message") or {}
            c = msg.get("content")
            if isinstance(c, str):
                return c
    return ""


def transcribe(model: str, key: str, audio_secs: float, ref: str) -> dict:
    HYP.mkdir(exist_ok=True)
    LOG.mkdir(exist_ok=True)
    safe = re.sub(r"[^\w.-]+", "_", model)
    hyp_path = HYP / f"or_direct_{safe}.txt"
    log_path = LOG / f"or_direct_{safe}.log"

    url = f"{API}/audio/transcriptions"
    headers = {
        "Authorization": f"Bearer {key}",
        "HTTP-Referer": "https://github.com/joe-broadhead/aurum",
        "X-Title": "aurum-plaud-bench",
    }
    print(f"\n== openrouter-direct/{model} ==", flush=True)
    t0 = time.perf_counter()
    try:
        with AUDIO.open("rb") as f:
            files = {"file": ("plaud_audio.mp3", f, "audio/mpeg")}
            data = {"model": model, "response_format": "json"}
            resp = requests.post(url, headers=headers, files=files, data=data, timeout=1800)
        elapsed = time.perf_counter() - t0
        body = resp.text
        # redact
        body_safe = re.sub(r"sk-or-v1-[A-Za-z0-9]+", "[REDACTED]", body)
        log_path.write_text(f"status={resp.status_code}\nelapsed={elapsed}\n{body_safe[:50000]}\n")
        if resp.status_code >= 400:
            err = body_safe[:300].replace("\n", " ")
            row = {
                "label": f"openrouter-direct/{model}",
                "provider": "openrouter-direct",
                "model": model,
                "status": "fail",
                "rc": resp.status_code,
                "wall_s": round(elapsed, 2),
                "rtf": round(elapsed / audio_secs, 4),
                "error_snippet": err,
                "ts_utc": datetime.now(timezone.utc).isoformat(),
            }
            print(f"  FAIL {resp.status_code} {err[:160]}", flush=True)
            return row
        try:
            payload = resp.json()
        except Exception:
            payload = body
        text = extract_text(payload).strip()
        hyp_path.write_text(text + ("\n" if text else ""))
        m = metrics(ref, text)
        ok = bool(text)
        row = {
            "label": f"openrouter-direct/{model}",
            "provider": "openrouter-direct",
            "model": model,
            "status": "pass" if ok else "fail",
            "rc": resp.status_code,
            "wall_s": round(elapsed, 2),
            "rtf": round(elapsed / audio_secs, 4),
            "hyp_chars": len(text),
            "hyp_path": str(hyp_path.relative_to(ROOT)),
            **m,
            "error_snippet": None if ok else "empty transcript",
            "ts_utc": datetime.now(timezone.utc).isoformat(),
        }
        wer_s = f"{row['wer']*100:.1f}%" if row.get("wer") is not None and ok else "n/a"
        print(f"  status={row['status']} wall={row['wall_s']}s wer={wer_s}", flush=True)
        return row
    except Exception as e:
        elapsed = time.perf_counter() - t0
        row = {
            "label": f"openrouter-direct/{model}",
            "provider": "openrouter-direct",
            "model": model,
            "status": "fail",
            "rc": -1,
            "wall_s": round(elapsed, 2),
            "rtf": round(elapsed / audio_secs, 4) if audio_secs else None,
            "error_snippet": str(e)[:240],
            "ts_utc": datetime.now(timezone.utc).isoformat(),
        }
        print(f"  EXC {e}", flush=True)
        return row


def write_report(rows: list[dict], meta: dict) -> None:
    def sk(r):
        if r.get("status") != "pass" or r.get("wer") is None:
            return (1, 9e9, r.get("label", ""))
        return (0, r["wer"], r.get("label", ""))

    rows_sorted = sorted(rows, key=sk)
    lines = [
        "# Plaud vs Aurum / OpenRouter transcription bench",
        "",
        f"- **generated_at_utc:** {meta['generated_at_utc']}",
        f"- **source:** Plaud recording `{meta['file_id']}` — *{meta['title']}*",
        f"- **audio:** {meta['audio_secs']:.1f}s MP3 ({meta['audio_bytes']} bytes)",
        f"- **reference:** Plaud transcript (speaker labels/timestamps stripped) — **{meta['ref_words']} words**",
        f"- **aurum:** {meta.get('aurum_version', '0.0.17')} local whisper.cpp path",
        f"- **openrouter path:** direct `POST /api/v1/audio/transcriptions` "
        f"(Aurum CLI rejected full-length single-segment responses at 8 000 chars/segment — product limit)",
        "",
        "> WER/CER measure **agreement with Plaud**, not absolute human ground truth. "
        "Plaud may already use commercial ASR + cleanup.",
        "",
        "## Leaderboard (best WER first)",
        "",
        "| Rank | System | Model | Status | WER ↓ | CER | Wall s | RTF |",
        "|-----:|--------|-------|--------|------:|----:|-------:|----:|",
    ]
    for i, r in enumerate(rows_sorted, 1):
        wer_s = f"{r['wer']*100:.2f}%" if r.get("wer") is not None and r["status"] == "pass" else "—"
        cer_s = f"{r['cer']*100:.2f}%" if r.get("cer") is not None and r["status"] == "pass" else "—"
        rtf = r.get("rtf")
        rtf_s = f"{rtf:.4f}" if isinstance(rtf, (int, float)) else "—"
        lines.append(
            f"| {i} | {r.get('provider')} | `{r.get('model')}` | {r.get('status')} | "
            f"{wer_s} | {cer_s} | {r.get('wall_s')} | {rtf_s} |"
        )

    # sections
    lines += ["", "## Aurum local (whisper.cpp)", ""]
    for r in rows_sorted:
        if r.get("provider") != "local":
            continue
        if r.get("status") == "pass" and r.get("wer") is not None:
            lines.append(
                f"- **`{r['model']}`** — WER {r['wer']*100:.2f}%, CER {r['cer']*100:.2f}%, "
                f"{r['wall_s']}s (RTF {r['rtf']})"
            )
        else:
            lines.append(f"- **`{r['model']}`** — FAIL: {r.get('error_snippet')}")

    lines += ["", "## OpenRouter `output_modalities=transcription`", ""]
    for r in rows_sorted:
        if r.get("provider") not in ("openrouter", "openrouter-direct"):
            continue
        if r.get("status") == "pass" and r.get("wer") is not None:
            lines.append(
                f"- **`{r['model']}`** — WER {r['wer']*100:.2f}%, {r['wall_s']}s"
            )
        else:
            lines.append(f"- **`{r['model']}`** — FAIL: {r.get('error_snippet')}")

    lines += [
        "",
        "## Notes",
        "",
        "- Local `large-v3` (full) failed residency budget: weight ~4.3 GiB > default max ~3.0 GiB.",
        "- Aurum CLI OpenRouter path returned the remote text but **failed closed** on "
        "`max_segment_chars=8000` for this ~11 min lecture (single segment).",
        "- Direct OpenRouter API used for fair quality comparison of the 13 transcription models.",
        "- Hypotheses under `hypotheses/`; row data in `results.jsonl` + `results_openrouter.jsonl`.",
        "",
    ]
    REPORT.write_text("\n".join(lines) + "\n")
    print(f"\nWrote {REPORT}", flush=True)


def main() -> int:
    key = os.environ.get("OPENROUTER_API_KEY", "").strip()
    if not key:
        print("OPENROUTER_API_KEY required", file=sys.stderr)
        return 1
    ref = REF_PATH.read_text()
    audio_secs = 685.08
    try:
        import subprocess

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
        audio_secs = float(json.loads(p.stdout)["format"]["duration"])
    except Exception:
        pass

    if RESULTS_OR.exists():
        RESULTS_OR.rename(RESULTS_OR.with_suffix(".jsonl.bak"))

    rows_or: list[dict] = []
    only = os.environ.get("OR_ONLY", "").strip()
    models = [only] if only else MODELS
    for model in models:
        row = transcribe(model, key, audio_secs, ref)
        rows_or.append(row)
        with RESULTS_OR.open("a") as f:
            f.write(json.dumps(row) + "\n")

    # merge local passes/fails
    rows: list[dict] = []
    if RESULTS_LOCAL.exists():
        for line in RESULTS_LOCAL.read_text().splitlines():
            if not line.strip():
                continue
            r = json.loads(line)
            if r.get("provider") == "local":
                rows.append(r)
    rows.extend(rows_or)

    meta = {
        "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "file_id": "42e86284875bf900d98cb7e83ce3b7cb",
        "title": "07-30 Lecture: AI-Driven Code Generation and Prototyping",
        "audio_secs": audio_secs,
        "audio_bytes": AUDIO.stat().st_size,
        "ref_words": len(normalize(ref).split()),
        "aurum_version": "aurum 0.0.17",
    }
    write_report(rows, meta)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
