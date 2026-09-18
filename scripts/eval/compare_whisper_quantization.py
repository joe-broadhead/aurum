#!/usr/bin/env python3
"""Compare pinned F16/Q5 whisper.cpp artifacts on referenced dialect clips."""

from __future__ import annotations

import argparse
import json
import subprocess
import time
from pathlib import Path

from run_portuguese_stt_eval import (
    error_rate,
    normalize,
    repetition_ratio,
    sha256_file,
)


def parse_run(value: str) -> tuple[str, str, Path]:
    parts = value.split(",", 2)
    if len(parts) != 3 or parts[1] not in {"pt-BR", "pt-PT"}:
        raise argparse.ArgumentTypeError("RUN must be LABEL,pt-BR|pt-PT,MODEL_PATH")
    return parts[0], parts[1], Path(parts[2]).resolve()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--whisper-cli", required=True)
    parser.add_argument(
        "--run",
        action="append",
        type=parse_run,
        required=True,
        help="LABEL,pt-BR|pt-PT,MODEL_PATH (repeat for every artifact)",
    )
    parser.add_argument("--out-dir", default="/tmp/aurum-portuguese-results/spotcheck")
    args = parser.parse_args()

    manifest_path = Path(args.manifest).resolve()
    whisper_cli = Path(args.whisper_cli).resolve()
    out_dir = Path(args.out_dir).resolve()
    if not whisper_cli.is_file():
        raise SystemExit(f"whisper-cli not found: {whisper_cli}")
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    fixtures = {
        dialect: next(item for item in manifest["items"] if item["dialect"] == dialect)
        for dialect in ("pt-BR", "pt-PT")
    }

    results = []
    for label, dialect, model_path in args.run:
        if not model_path.is_file():
            raise SystemExit(f"model not found: {model_path}")
        fixture = fixtures[dialect]
        audio = (manifest_path.parent / fixture["audio"]).resolve()
        output_prefix = out_dir / label
        command = [
            str(whisper_cli),
            "-m",
            str(model_path),
            "-f",
            str(audio),
            "-l",
            "pt",
            "-nt",
            "-np",
            "-otxt",
            "-of",
            str(output_prefix),
        ]
        started = time.perf_counter()
        process = subprocess.run(command, capture_output=True, text=True)
        wall_s = time.perf_counter() - started
        transcript_path = Path(f"{output_prefix}.txt")
        hypothesis = (
            transcript_path.read_text(encoding="utf-8", errors="replace").strip()
            if process.returncode == 0 and transcript_path.is_file()
            else ""
        )
        word_errors, reference_words, wer = error_rate(
            fixture["reference"], hypothesis, words=True
        )
        char_errors, reference_chars, cer = error_rate(
            fixture["reference"], hypothesis, words=False
        )
        row = {
            "label": label,
            "dialect": dialect,
            "fixture_id": fixture["id"],
            "model_path": str(model_path),
            "model_bytes": model_path.stat().st_size,
            "model_sha256": sha256_file(model_path),
            "status": "ok" if process.returncode == 0 else "error",
            "exit_code": process.returncode,
            "error": process.stderr[-2000:] if process.returncode else None,
            "reference": fixture["reference"],
            "hypothesis": hypothesis,
            "normalized_hypothesis": normalize(hypothesis),
            "word_errors": word_errors,
            "reference_words": reference_words,
            "wer": round(wer, 6),
            "char_errors": char_errors,
            "reference_chars": reference_chars,
            "cer": round(cer, 6),
            "duration_s": fixture["duration_s"],
            "wall_s": round(wall_s, 3),
            "rtf": round(wall_s / fixture["duration_s"], 6),
            "repetition_ratio": round(repetition_ratio(hypothesis), 6),
        }
        results.append(row)
        print(
            f"{label}: status={row['status']} wer={row['wer']} "
            f"cer={row['cer']} rtf={row['rtf']}"
        )

    comparisons = []
    by_dialect: dict[str, list[dict]] = {"pt-BR": [], "pt-PT": []}
    for row in results:
        by_dialect[row["dialect"]].append(row)
    for dialect, rows in by_dialect.items():
        if len(rows) == 2:
            comparisons.append(
                {
                    "dialect": dialect,
                    "labels": [row["label"] for row in rows],
                    "normalized_transcripts_equal": rows[0]["normalized_hypothesis"]
                    == rows[1]["normalized_hypothesis"],
                    "wer_delta_second_minus_first": round(rows[1]["wer"] - rows[0]["wer"], 6),
                    "cer_delta_second_minus_first": round(rows[1]["cer"] - rows[0]["cer"], 6),
                }
            )

    report = {
        "schema_version": 1,
        "language": "pt",
        "whisper_cli": str(whisper_cli),
        "manifest": str(manifest_path),
        "results": results,
        "comparisons": comparisons,
    }
    output = out_dir / "quantization-spotcheck.json"
    output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(f"WROTE {output}")


if __name__ == "__main__":
    main()
