#!/usr/bin/env python3
"""Compare pinned F16/Q5 whisper.cpp artifacts on referenced dialect clips."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import time
from pathlib import Path

from run_portuguese_stt_eval import (
    classify_transcript,
    error_rate,
    fresh_run_dir,
    normalize,
    repetition_ratio,
    sha256_file,
    user_cache_root,
)


def parse_run(value: str) -> tuple[str, str, Path]:
    parts = value.split(",", 2)
    if len(parts) != 3 or parts[1] not in {"pt-BR", "pt-PT"}:
        raise argparse.ArgumentTypeError("RUN must be LABEL,pt-BR|pt-PT,MODEL_PATH")
    return parts[0], parts[1], Path(parts[2]).resolve()


def select_fixtures(
    manifest_path: Path, manifest: dict
) -> dict[str, tuple[dict, Path]]:
    """Return the first referenced fixture and audio path for each dialect."""
    fixtures: dict[str, tuple[dict, Path]] = {}
    for dialect in ("pt-BR", "pt-PT"):
        matches = [
            item for item in manifest.get("items", []) if item.get("dialect") == dialect
        ]
        if not matches:
            raise SystemExit(f"manifest has no {dialect} fixture: {manifest_path}")
        fixture = matches[0]
        audio = (manifest_path.parent / fixture["audio"]).resolve()
        if not audio.is_file():
            raise SystemExit(f"missing audio fixture: {audio}")
        fixtures[dialect] = (fixture, audio)
    return fixtures


def build_comparison(dialect: str, rows: list[dict]) -> dict | None:
    """Build a dialect comparison only when both runs actually succeeded."""
    if len(rows) != 2:
        return None
    if any(row.get("status") != "ok" for row in rows):
        return None
    return {
        "dialect": dialect,
        "labels": [row["label"] for row in rows],
        "normalized_transcripts_equal": (
            rows[0]["normalized_hypothesis"] == rows[1]["normalized_hypothesis"]
        ),
        "wer_delta_second_minus_first": round(rows[1]["wer"] - rows[0]["wer"], 6),
        "cer_delta_second_minus_first": round(rows[1]["cer"] - rows[0]["cer"], 6),
    }


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
    parser.add_argument(
        "--out-dir",
        default=str(user_cache_root() / "portuguese-results" / "spotcheck"),
    )
    args = parser.parse_args()

    manifest_path = Path(args.manifest).resolve()
    whisper_cli = Path(args.whisper_cli).resolve()
    out_dir = Path(args.out_dir).resolve()
    if not manifest_path.is_file():
        raise SystemExit(f"manifest not found: {manifest_path}")
    if not whisper_cli.is_file():
        raise SystemExit(f"whisper-cli not found: {whisper_cli}")
    if not os.access(whisper_cli, os.X_OK):
        raise SystemExit(f"whisper-cli is not executable: {whisper_cli}")
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    fixtures = select_fixtures(manifest_path, manifest)

    results = []
    for label, dialect, model_path in args.run:
        if not model_path.is_file():
            raise SystemExit(f"model not found: {model_path}")
        fixture, audio = fixtures[dialect]
        # Every invocation gets its own private directory, so a transcript left
        # by an earlier run can never be mistaken for this run's output.
        run_dir = fresh_run_dir(out_dir, f"{label}-{dialect}")
        output_prefix = run_dir / "transcript"
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
        status, hypothesis, error = classify_transcript(
            process.returncode, process.stderr, transcript_path
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
            "status": status,
            "exit_code": process.returncode,
            "error": error,
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
        comparison = build_comparison(dialect, rows)
        if comparison is None:
            print(f"{dialect}: comparison skipped (a run did not succeed)")
            continue
        comparisons.append(comparison)

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
