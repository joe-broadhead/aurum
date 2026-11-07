#!/usr/bin/env python3
"""Dictation eval corpus harness (#11).

Two modes:

  --check   Offline structural validation: the manifest parses, every clip and
            reference is present, durations match their declared range, the
            SHA256SUMS file matches the audio, and the licence is recorded.
            No model, no network. This is what CI runs on every change.

  (default) Run the built `aurum` binary on each clip and score WER, CER,
            vocabulary recall, silence false positives and real-time factor.
            Fails (exit 1) when the corpus budget is exceeded.

The corpus is dictation-length (1-20 s), so it measures what a keyboard
replacement actually hears, not a lecture. The budget is a floor for
regressions, not a claim about state-of-the-art quality.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import time
from pathlib import Path

NORMALIZATION = "aurum.dictation.normalize.v1"
MIN_SECONDS = 1.0
MAX_SECONDS = 20.0


def norm_words(s: str) -> list[str]:
    s = s.lower()
    s = re.sub(r"[^a-z0-9\s']", " ", s)
    s = s.replace("'", "")
    return s.split()


def edit_distance(ref: list[str], hyp: list[str]) -> int:
    prev = list(range(len(hyp) + 1))
    for i, rw in enumerate(ref, 1):
        curr = [i]
        for j, hw in enumerate(hyp, 1):
            cost = 0 if rw == hw else 1
            curr.append(min(prev[j] + 1, curr[j - 1] + 1, prev[j - 1] + cost))
        prev = curr
    return prev[-1]


def wer(ref: str, hyp: str) -> tuple[float, int, int]:
    r, h = norm_words(ref), norm_words(hyp)
    if not r:
        return (0.0 if not h else 1.0, 0, len(h))
    return (edit_distance(r, h) / len(r), len(r), len(h))


def cer(ref: str, hyp: str) -> float:
    r, h = list(" ".join(norm_words(ref))), list(" ".join(norm_words(hyp)))
    if not r:
        return 0.0 if not h else 1.0
    return edit_distance(r, h) / len(r)


def vocab_recall(terms: list[str], hyp: str) -> tuple[int, int]:
    """Count expected terms that survive into the hypothesis."""
    if not terms:
        return (0, 0)
    tokens = norm_words(hyp)
    hit = 0
    for term in terms:
        t = norm_words(term)
        if not t:
            continue
        n = len(t)
        if any(tokens[i : i + n] == t for i in range(len(tokens) - n + 1)):
            hit += 1
    return (hit, len(terms))


def wav_seconds(path: Path) -> float:
    import wave

    with wave.open(str(path), "rb") as w:
        return w.getnframes() / float(w.getframerate())


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def load_corpus(corpus_path: Path) -> dict:
    doc = json.loads(corpus_path.read_text(encoding="utf-8"))
    if doc.get("normalization") != NORMALIZATION:
        raise SystemExit(
            f"corpus normalization policy mismatch: {doc.get('normalization')!r} != {NORMALIZATION!r}"
        )
    return doc


def check(corpus_path: Path, doc: dict) -> list[str]:
    problems: list[str] = []
    root = corpus_path.parent
    seen = set()
    for clip in doc.get("clips", []):
        cid = clip.get("id", "")
        if not cid or cid in seen:
            problems.append(f"bad or duplicate clip id: {cid!r}")
        seen.add(cid)
        if not clip.get("license"):
            problems.append(f"{cid}: no licence recorded")
        audio = root / clip.get("audio", "")
        if not clip.get("audio") or not audio.is_file():
            problems.append(f"{cid}: missing audio {audio}")
            continue
        secs = wav_seconds(audio)
        if not (MIN_SECONDS - 0.05 <= secs <= MAX_SECONDS + 0.05):
            problems.append(f"{cid}: {secs:.2f}s outside {MIN_SECONDS}-{MAX_SECONDS}s")
        if "reference" not in clip:
            problems.append(f"{cid}: missing reference")
        if not clip.get("reference") and "silence" not in clip.get("tags", []):
            problems.append(f"{cid}: empty reference without a silence tag")
    if len(doc.get("clips", [])) < 12:
        problems.append(f"corpus has {len(doc.get('clips', []))} clips; at least 12 are required")

    sums = root / "SHA256SUMS"
    if not sums.is_file():
        problems.append("missing SHA256SUMS")
    else:
        listed = {}
        for line in sums.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            digest, name = line.split(maxsplit=1)
            listed[name.strip()] = digest
        for clip in doc.get("clips", []):
            rel = clip.get("audio", "")
            if rel not in listed:
                problems.append(f"{rel}: not in SHA256SUMS")
                continue
            actual = sha256_file(root / rel)
            if actual != listed[rel]:
                problems.append(f"{rel}: sha256 mismatch (run the generator)")
    return problems


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default="evals/dictation/corpus.v1.json")
    ap.add_argument("--check", action="store_true", help="offline structural validation only")
    ap.add_argument("--aurum", default=None)
    ap.add_argument("--model", default="tiny-q5_1")
    ap.add_argument("--cache-dir", default=None)
    ap.add_argument("--language", default="en")
    ap.add_argument("--out", default=None)
    ap.add_argument("--json", action="store_true", help="print the report to stdout")
    args = ap.parse_args()

    corpus_path = Path(args.corpus)
    doc = load_corpus(corpus_path)
    problems = check(corpus_path, doc)

    if args.check:
        if problems:
            print("dictation corpus: FAIL")
            for p in problems:
                print(f"  - {p}")
            return 1
        total = sum(wav_seconds(corpus_path.parent / c["audio"]) for c in doc["clips"])
        print(
            f"dictation corpus ok: {len(doc['clips'])} clips, {total:.1f}s total, "
            f"{doc['corpus_license']}"
        )
        return 0

    if problems:
        for p in problems:
            print(f"corpus problem: {p}", file=sys.stderr)
        return 1

    aurum = Path(args.aurum or "target/release/aurum")
    if not aurum.is_file():
        # Fall back to whatever is on PATH (an installed release).
        from shutil import which

        found = which("aurum")
        if not found:
            raise SystemExit(f"aurum binary not found: {aurum} (build with make build)")
        aurum = Path(found)

    budget = doc.get("budget", {})
    results = []
    vocab_hit = vocab_total = 0
    silence_fp = 0
    silence_words = 0

    for clip in doc["clips"]:
        audio = (corpus_path.parent / clip["audio"]).resolve()
        cmd = [
            str(aurum),
            str(audio),
            "--model",
            args.model,
            "--language",
            args.language,
            "--output",
            "txt",
        ]
        if args.cache_dir:
            cmd.extend(["--cache-dir", args.cache_dir])
        started = time.time()
        proc = subprocess.run(cmd, capture_output=True, text=True)
        elapsed = time.time() - started
        if proc.returncode != 0:
            hyp = ""
            error = proc.stderr.strip().splitlines()[-1] if proc.stderr.strip() else "aurum failed"
        else:
            hyp = proc.stdout.strip()
            error = None
        ref = clip.get("reference", "")
        w, ref_words, hyp_words = wer(ref, hyp)
        if not norm_words(ref):
            inserted = len(norm_words(hyp))
            if inserted:
                silence_fp += 1
                silence_words += inserted
        hit, terms = vocab_recall(clip.get("vocabulary", []), hyp)
        vocab_hit += hit
        vocab_total += terms
        secs = wav_seconds(audio)
        row = {
            "id": clip["id"],
            "tags": clip.get("tags", []),
            "seconds": round(secs, 3),
            "wer": round(w, 4),
            "cer": round(cer(ref, hyp), 4),
            "ref_words": ref_words,
            "hyp_words": hyp_words,
            "vocabulary_hit": hit,
            "vocabulary_total": terms,
            "rtf": round(elapsed / secs, 4) if secs else None,
            "wall_s": round(elapsed, 3),
            "hypothesis": hyp,
            "reference": ref,
        }
        if error:
            row["error"] = error
        results.append(row)
        print(
            f"{clip['id']:22s} wer={w:5.3f} cer={row['cer']:5.3f} "
            f"rtf={row['rtf'] if row['rtf'] is not None else float('nan'):5.2f} "
            f"{'ERROR: ' + error if error else ''}"
        )

    speech = [r for r in results if norm_words(r["reference"])]
    mean_wer = sum(r["wer"] for r in speech) / len(speech) if speech else 1.0
    mean_cer = sum(r["cer"] for r in speech) / len(speech) if speech else 1.0
    recall = (vocab_hit / vocab_total) if vocab_total else 1.0
    rtfs = [r["rtf"] for r in speech if r["rtf"] is not None]
    report = {
        "schema_version": 1,
        "corpus": doc["name"],
        "normalization": NORMALIZATION,
        "model": args.model,
        "aurum": str(aurum),
        "clips": len(results),
        "speech_clips": len(speech),
        "total_audio_seconds": round(sum(r["seconds"] for r in results), 3),
        "mean_wer": round(mean_wer, 4),
        "mean_cer": round(mean_cer, 4),
        "vocabulary_recall": round(recall, 4),
        "silence_false_positives": silence_fp,
        "silence_inserted_words": silence_words,
        "mean_rtf": round(sum(rtfs) / len(rtfs), 4) if rtfs else None,
        "budget": budget,
        "results": results,
    }

    failures = []
    if "mean_wer_max" in budget and mean_wer > budget["mean_wer_max"] + 1e-9:
        failures.append(f"mean WER {mean_wer:.3f} > budget {budget['mean_wer_max']}")
    if "vocabulary_recall_min" in budget and recall < budget["vocabulary_recall_min"] - 1e-9:
        failures.append(
            f"vocabulary recall {recall:.3f} < budget {budget['vocabulary_recall_min']}"
        )
    if (
        "silence_false_positives_max" in budget
        and silence_fp > budget["silence_false_positives_max"]
    ):
        failures.append(
            f"silence false positives {silence_fp} > budget {budget['silence_false_positives_max']}"
        )
    if (
        "silence_inserted_words_max" in budget
        and silence_words > budget["silence_inserted_words_max"]
    ):
        failures.append(
            f"silence hallucinated {silence_words} words "
            f"> budget {budget['silence_inserted_words_max']}"
        )
    for r in speech:
        cap = budget.get("word_error_rate_max_any_clip")
        if cap is not None and r["wer"] > cap + 1e-9:
            failures.append(f"{r['id']}: WER {r['wer']:.3f} > per-clip cap {cap}")
    report["failures"] = failures
    report["ok"] = not failures

    print(
        f"\n{report['corpus']} model={args.model}: mean WER {mean_wer:.3f} "
        f"CER {mean_cer:.3f} vocab recall {recall:.3f} "
        f"silence FP {silence_fp} ({silence_words} words) mean RTF {report['mean_rtf']}"
    )

    out = Path(args.out) if args.out else Path("evals/reports/_local") / f"dictation-{args.model}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"report: {out}")

    if args.json:
        print(json.dumps(report, indent=2))

    if failures:
        print("FAILED:")
        for f in failures:
            print(f"  - {f}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
