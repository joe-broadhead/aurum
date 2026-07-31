#!/usr/bin/env python3
"""Offline STT evaluation matrix (JOE-1731).

Requires a built `aurum` binary (set AURUM_BIN) and cached models.
Writes machine-readable reports under evals/reports/stt/.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import subprocess
import time
from pathlib import Path


def norm(s: str) -> str:
    s = s.lower()
    s = re.sub(r"[^a-z0-9\s]", " ", s)
    return " ".join(s.split())


def lev_rate(ref: str, hyp: str, unit: str = "word") -> float:
    if unit == "word":
        r, h = norm(ref).split(), norm(hyp).split()
    else:
        r, h = list(norm(ref)), list(norm(hyp))
    if not r:
        return 0.0 if not h else 1.0
    prev = list(range(len(h) + 1))
    for i, rw in enumerate(r, 1):
        curr = [i]
        for j, hw in enumerate(h, 1):
            cost = 0 if rw == hw else 1
            curr.append(min(prev[j] + 1, curr[j - 1] + 1, prev[j - 1] + cost))
        prev = curr
    return prev[-1] / len(r)


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default="evals/corpus.public-v1.json")
    ap.add_argument("--out-dir", default="evals/reports/stt")
    ap.add_argument("--profile", default="apple_silicon_metal")
    ap.add_argument("--models", default="tiny-q5_1,base,small-q5_1")
    ap.add_argument("--aurum", default=os.environ.get("AURUM_BIN", "target/release/aurum"))
    args = ap.parse_args()

    corpus_path = Path(args.corpus)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    aurum = Path(args.aurum)
    if not aurum.is_file():
        raise SystemExit(f"aurum binary not found: {aurum} (build with cargo build -p aurum-stt --release)")

    corpus = json.loads(corpus_path.read_text(encoding="utf-8"))
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    version = Path("VERSION").read_text(encoding="utf-8").strip()
    models = [m.strip() for m in args.models.split(",") if m.strip()]

    for model in models:
        scores = []
        print(f"=== model {model} ===")
        for fix in corpus.get("stt", []):
            if not fix.get("audio"):
                continue
            fid = fix["id"]
            audio = (corpus_path.parent / fix["audio"]).resolve()
            if not audio.is_file():
                print(f"  MISSING {audio}")
                continue
            out_txt = out_dir / f"_tmp_{model}_{fid}.txt"
            t0 = time.time()
            r = subprocess.run(
                [
                    str(aurum),
                    str(audio),
                    "--model",
                    model,
                    "--output-file",
                    str(out_txt),
                    "--language",
                    "en",
                ],
                capture_output=True,
                text=True,
            )
            elapsed = time.time() - t0
            if r.returncode != 0:
                print(f"  FAIL {fid}")
                scores.append(
                    {
                        "fixture_id": fid,
                        "error": "aurum failed",
                        "wer": 1.0,
                        "cer": 1.0,
                        "silence_false_positive": False,
                        "repetition_ratio": 0.0,
                        "hypothesis": "",
                    }
                )
                continue
            hyp = out_txt.read_text(encoding="utf-8", errors="replace").strip()
            out_txt.unlink(missing_ok=True)
            ref = fix.get("reference", "")
            sfp = norm(ref) == "" and norm(hyp) != ""
            words = norm(hyp).split()
            rep = 0.0
            if words:
                best = run = 1
                for a, b in zip(words, words[1:]):
                    run = run + 1 if a == b else 1
                    best = max(best, run)
                rep = best / len(words)
            w = round(lev_rate(ref, hyp, "word"), 4)
            scores.append(
                {
                    "fixture_id": fid,
                    "wer": w,
                    "cer": round(lev_rate(ref, hyp, "char"), 4),
                    "silence_false_positive": sfp,
                    "repetition_ratio": round(rep, 4),
                    "hypothesis": hyp,
                    "reference": ref,
                    "tags": fix.get("tags", []),
                    "audio_sha256": sha256_file(audio),
                    "wall_s": round(elapsed, 3),
                }
            )
            print(f"  {fid}: wer={w} wall={elapsed:.1f}s")

        speech_scores = [
            s
            for s in scores
            if not any(t in s.get("tags", []) for t in ("silence", "non_speech", "music"))
        ]
        n = max(len(scores), 1)
        ns = max(len(speech_scores), 1)
        report = {
            "schema_version": 1,
            "corpus_version": corpus.get("version"),
            "corpus_name": corpus.get("name"),
            "model": model,
            "backend_kind": "asr",
            "provider": "local",
            "hardware_profile": args.profile,
            "host": "maintainer-profile-host",
            "os": platform.platform(),
            "machine": platform.machine(),
            "aurum_version": version,
            "commit": commit,
            "stt_scores": scores,
            "mean_wer": round(sum(s["wer"] for s in scores) / n, 4),
            "mean_cer": round(sum(s["cer"] for s in scores) / n, 4),
            "mean_wer_speech_only": round(sum(s["wer"] for s in speech_scores) / ns, 4),
            "silence_false_positives": sum(1 for s in scores if s.get("silence_false_positive")),
            "mean_repetition_ratio": round(sum(s["repetition_ratio"] for s in scores) / n, 4),
            "notes": "synthetic speech corpus; silence/non-speech included in mean_wer",
        }
        out = out_dir / f"stt-{args.profile}-{model}.json"
        out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(
            f"WROTE {out} mean_wer={report['mean_wer']} speech_only={report['mean_wer_speech_only']} sfp={report['silence_false_positives']}"
        )


if __name__ == "__main__":
    main()
