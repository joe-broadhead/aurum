#!/usr/bin/env python3
"""Run the pinned Portuguese STT corpus through a local Aurum model matrix."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import subprocess
import time
import unicodedata
from collections import Counter
from pathlib import Path


DEFAULT_MODELS = (
    "medium-ptbr-q5_0,large-v3-ptpt-q5_0,large-v3-turbo-q5_0"
)


def normalize(text: str) -> str:
    chars = []
    for char in unicodedata.normalize("NFC", text.casefold()):
        category = unicodedata.category(char)
        chars.append(char if category[0] in {"L", "N"} else " ")
    return " ".join("".join(chars).split())


def edit_distance(reference: list[str], hypothesis: list[str]) -> int:
    previous = list(range(len(hypothesis) + 1))
    for row, ref_unit in enumerate(reference, 1):
        current = [row]
        for column, hyp_unit in enumerate(hypothesis, 1):
            substitution = previous[column - 1] + (ref_unit != hyp_unit)
            current.append(min(previous[column] + 1, current[column - 1] + 1, substitution))
        previous = current
    return previous[-1]


def error_rate(reference: str, hypothesis: str, words: bool) -> tuple[int, int, float]:
    ref_norm = normalize(reference)
    hyp_norm = normalize(hypothesis)
    ref_units = ref_norm.split() if words else list(ref_norm.replace(" ", ""))
    hyp_units = hyp_norm.split() if words else list(hyp_norm.replace(" ", ""))
    errors = edit_distance(ref_units, hyp_units)
    denominator = len(ref_units)
    rate = errors / denominator if denominator else (0.0 if not hyp_units else 1.0)
    return errors, denominator, rate


def repetition_ratio(text: str) -> float:
    words = normalize(text).split()
    if not words:
        return 0.0
    bigrams = list(zip(words, words[1:]))
    if not bigrams:
        return 0.0
    repeats = sum(count - 1 for count in Counter(bigrams).values() if count > 1)
    return repeats / len(bigrams)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def aggregate(rows: list[dict]) -> dict:
    completed = [row for row in rows if row["status"] == "ok"]
    word_errors = sum(row["word_errors"] for row in completed)
    word_count = sum(row["reference_words"] for row in completed)
    char_errors = sum(row["char_errors"] for row in completed)
    char_count = sum(row["reference_chars"] for row in completed)
    audio_s = sum(row["duration_s"] for row in completed)
    wall_s = sum(row["wall_s"] for row in completed)
    return {
        "clips": len(rows),
        "completed": len(completed),
        "errors": len(rows) - len(completed),
        "empty_results": sum(1 for row in completed if not normalize(row["hypothesis"])),
        "wer": round(word_errors / word_count, 6) if word_count else None,
        "cer": round(char_errors / char_count, 6) if char_count else None,
        "wall_s": round(wall_s, 3),
        "audio_s": round(audio_s, 3),
        "rtf": round(wall_s / audio_s, 6) if audio_s else None,
        "mean_repetition_ratio": round(
            sum(row["repetition_ratio"] for row in completed) / len(completed), 6
        )
        if completed
        else None,
    }


def markdown_report(report: dict) -> str:
    reused_rows = sum(1 for row in report["results"] if row.get("reused"))
    fresh_rows = len(report["results"]) - reused_rows
    lines = [
        "# Portuguese STT comparison",
        "",
        "Machine-readable evidence: `portuguese-stt-results.json`.",
        f"Rows: {len(report['results'])} total, {fresh_rows} fresh, "
        f"{reused_rows} SHA-validated reuse.",
        "",
        "| Model | Dialect | Clips | Errors | WER | CER | RTF | Repetition |",
        "|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for model, dialects in report["summary"].items():
        for dialect, values in dialects.items():
            lines.append(
                f"| `{model}` | {dialect} | {values['completed']}/{values['clips']} "
                f"| {values['errors']} | {values['wer']} | {values['cer']} "
                f"| {values['rtf']} | {values['mean_repetition_ratio']} |"
            )
    lines.extend(
        [
            "",
            "## Interpretation constraints",
            "",
            "- CAMOES may be in-domain for `large-v3-ptpt-q5_0`.",
            "- CORAA declares no repository license; selected audio remains local-only.",
            "- Review hypotheses manually for hallucinations, names, numbers, punctuation, and dialect vocabulary.",
            "- Aggregate RTF may combine reused and fresh sequential runs; compare absolute performance only under matching host load.",
            "- Do not claim WER for private audio without reference transcripts.",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--aurum", required=True)
    parser.add_argument("--models", default=DEFAULT_MODELS)
    parser.add_argument("--out-dir", default="/tmp/aurum-portuguese-results")
    parser.add_argument("--profile", default="local-host")
    parser.add_argument(
        "--reuse-results",
        action="append",
        default=[],
        help="reuse successful rows when model, audio SHA, and reference match",
    )
    args = parser.parse_args()

    manifest_path = Path(args.manifest).resolve()
    aurum = Path(args.aurum).resolve()
    out_dir = Path(args.out_dir).resolve()
    if not aurum.is_file():
        raise SystemExit(f"aurum binary not found: {aurum}")
    out_dir.mkdir(parents=True, exist_ok=True)
    transcript_dir = out_dir / "transcripts"
    transcript_dir.mkdir(exist_ok=True)

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    clips_per_dialect = manifest.get("selection", {}).get("clips_per_dialect")
    if not isinstance(clips_per_dialect, int) or clips_per_dialect < 1:
        raise SystemExit("manifest must declare a positive clips_per_dialect")
    expected_total = clips_per_dialect * 2
    if manifest.get("language") != "pt" or len(manifest.get("items", [])) != expected_total:
        raise SystemExit(
            f"manifest must contain exactly {expected_total} Portuguese clips"
        )
    dialect_counts = Counter(item.get("dialect") for item in manifest["items"])
    if dialect_counts != {
        "pt-BR": clips_per_dialect,
        "pt-PT": clips_per_dialect,
    }:
        raise SystemExit(f"manifest dialect counts are invalid: {dict(dialect_counts)}")
    models = [model.strip() for model in args.models.split(",") if model.strip()]
    if not models:
        raise SystemExit("at least one model is required")

    aurum_sha256 = sha256_file(aurum)
    reusable: dict[tuple[str, str, str], tuple[dict, str]] = {}
    for reuse_value in args.reuse_results:
        reuse_path = Path(reuse_value).resolve()
        prior = json.loads(reuse_path.read_text(encoding="utf-8"))
        if prior.get("provider") != "local" or prior.get("language") != "pt":
            raise SystemExit(f"incompatible reusable report: {reuse_path}")
        if prior.get("aurum_binary_sha256") != aurum_sha256:
            raise SystemExit(f"reusable report used a different Aurum binary: {reuse_path}")
        for row in prior.get("results", []):
            if row.get("status") == "ok":
                key = (
                    row.get("model"),
                    row.get("audio_sha256"),
                    row.get("reference"),
                )
                reusable[key] = (row, str(reuse_path))

    rows = []
    for model in models:
        for item in manifest["items"]:
            audio = (manifest_path.parent / item["audio"]).resolve()
            if not audio.is_file():
                raise SystemExit(f"missing audio fixture: {audio}")
            audio_sha256 = sha256_file(audio)
            reused = reusable.get((model, audio_sha256, item["reference"]))
            if reused is not None:
                prior_row, reuse_source = reused
                row = dict(prior_row)
                row["reused"] = True
                row["reused_from"] = reuse_source
                rows.append(row)
                print(
                    f"{model} {item['id']}: status=ok wer={row['wer']} "
                    f"rtf={row['rtf']} reused=true"
                )
                continue
            transcript = transcript_dir / f"{model}-{item['id']}.txt"
            command = [
                str(aurum),
                str(audio),
                "--model",
                model,
                "--language",
                "pt",
                "--output-file",
                str(transcript),
            ]
            started = time.perf_counter()
            process = subprocess.run(command, capture_output=True, text=True)
            wall_s = time.perf_counter() - started
            hypothesis = (
                transcript.read_text(encoding="utf-8", errors="replace").strip()
                if process.returncode == 0 and transcript.is_file()
                else ""
            )
            word_errors, reference_words, wer = error_rate(
                item["reference"], hypothesis, words=True
            )
            char_errors, reference_chars, cer = error_rate(
                item["reference"], hypothesis, words=False
            )
            row = {
                "model": model,
                "fixture_id": item["id"],
                "dialect": item["dialect"],
                "source_dataset": item["source_dataset"],
                "source_revision": item["source_revision"],
                "source_fixture_id": item["source_fixture_id"],
                "audio_sha256": audio_sha256,
                "duration_s": item["duration_s"],
                "status": "ok" if process.returncode == 0 else "error",
                "exit_code": process.returncode,
                "error": process.stderr[-2000:] if process.returncode else None,
                "reference": item["reference"],
                "hypothesis": hypothesis,
                "word_errors": word_errors,
                "reference_words": reference_words,
                "wer": round(wer, 6),
                "char_errors": char_errors,
                "reference_chars": reference_chars,
                "cer": round(cer, 6),
                "wall_s": round(wall_s, 3),
                "rtf": round(wall_s / item["duration_s"], 6),
                "repetition_ratio": round(repetition_ratio(hypothesis), 6),
                "qualitative_review": None,
                "reused": False,
                "reused_from": None,
            }
            rows.append(row)
            print(
                f"{model} {item['id']}: status={row['status']} "
                f"wer={row['wer']} rtf={row['rtf']}"
            )

    summary = {}
    for model in models:
        summary[model] = {}
        for dialect in ("pt-BR", "pt-PT"):
            summary[model][dialect] = aggregate(
                [row for row in rows if row["model"] == model and row["dialect"] == dialect]
            )

    report = {
        "schema_version": 1,
        "provider": "local",
        "language": "pt",
        "profile": args.profile,
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
        },
        "aurum_binary": str(aurum),
        "aurum_binary_sha256": aurum_sha256,
        "manifest": str(manifest_path),
        "manifest_sha256": sha256_file(manifest_path),
        "models": models,
        "reuse_sources": [str(Path(value).resolve()) for value in args.reuse_results],
        "summary": summary,
        "results": rows,
    }
    json_path = out_dir / "portuguese-stt-results.json"
    markdown_path = out_dir / "portuguese-stt-comparison.md"
    json_path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    markdown_path.write_text(markdown_report(report), encoding="utf-8")
    print(f"WROTE {json_path}")
    print(f"WROTE {markdown_path}")


if __name__ == "__main__":
    main()
