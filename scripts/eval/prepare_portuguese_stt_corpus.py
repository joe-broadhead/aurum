#!/usr/bin/env python3
"""Prepare a pinned, local-only pt-BR/pt-PT STT evaluation corpus.

Install runtime dependencies outside the repository, for example:
  uv venv --python 3.11 /tmp/aurum-portuguese-eval-venv
  uv pip install --python /tmp/aurum-portuguese-eval-venv/bin/python \
    datasets==2.19.2 huggingface-hub==0.28.1 soundfile==0.12.1

Audio and the generated manifest must remain outside Git.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import zipfile
from collections import defaultdict, deque
from pathlib import Path

from datasets import Audio, Dataset, concatenate_datasets, load_dataset
from huggingface_hub import hf_hub_download
import soundfile as sf


CORAA_REPO = "gabrielrstan/CORAA-v1.1"
CORAA_REVISION = "719c91226a79f5f9a8984145f15f29626eabc29a"
CAMOES_REPO = "inesc-id/camoes_SI"
CAMOES_REVISION = "c85aec9653738c2a58ead316b61e1438cf845bc5"
MIN_DURATION_S = 5.0
MAX_DURATION_S = 20.0
DEFAULT_CLIPS_PER_DIALECT = 10


def stable_key(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def safe_id(prefix: str, value: str) -> str:
    stem = "".join(c if c.isalnum() else "-" for c in value).strip("-")
    return f"{prefix}-{stem[-48:]}"


def find_zip_member(zf: zipfile.ZipFile, requested: str) -> str | None:
    normalized = requested.lstrip("./")
    names = zf.namelist()
    exact = {name.lstrip("./"): name for name in names}
    if normalized in exact:
        return exact[normalized]
    matches = [name for name in names if name.endswith("/" + normalized)]
    return matches[0] if len(matches) == 1 else None


def prepare_coraa(out_dir: Path, hf_cache: Path, clip_count: int) -> list[dict]:
    metadata_path = Path(
        hf_hub_download(
            repo_id=CORAA_REPO,
            repo_type="dataset",
            revision=CORAA_REVISION,
            filename="metadata_test_final.csv",
            cache_dir=hf_cache,
        )
    )
    archive_path = Path(
        hf_hub_download(
            repo_id=CORAA_REPO,
            repo_type="dataset",
            revision=CORAA_REVISION,
            filename="test.zip",
            cache_dir=hf_cache,
        )
    )
    with metadata_path.open(encoding="utf-8-sig", newline="") as handle:
        rows = list(csv.DictReader(handle))

    groups: dict[tuple[str, str], deque[dict]] = defaultdict(deque)
    for row in sorted(rows, key=lambda r: stable_key(r["file_path"])):
        groups[(row.get("dataset", ""), row.get("accent", ""))].append(row)

    selected: list[dict] = []
    with zipfile.ZipFile(archive_path) as zf:
        active = deque(sorted(groups))
        while active and len(selected) < clip_count:
            group = active.popleft()
            candidates = groups[group]
            accepted = False
            while candidates and not accepted:
                row = candidates.popleft()
                member = find_zip_member(zf, row["file_path"])
                if member is None:
                    continue
                with zf.open(member) as source:
                    raw = source.read()
                try:
                    info = sf.info(io.BytesIO(raw))
                    duration = info.frames / info.samplerate
                except RuntimeError:
                    continue
                if not MIN_DURATION_S <= duration <= MAX_DURATION_S:
                    continue
                fixture_id = safe_id("ptbr", row["file_path"])
                audio_path = out_dir / f"{fixture_id}.wav"
                samples, sample_rate = sf.read(
                    io.BytesIO(raw), dtype="float32", always_2d=False
                )
                sf.write(audio_path, samples, sample_rate, subtype="PCM_16")
                selected.append(
                    {
                        "id": fixture_id,
                        "dialect": "pt-BR",
                        "audio": audio_path.name,
                        "reference": row["text"].strip(),
                        "duration_s": round(duration, 4),
                        "speaker_id": None,
                        "source_dataset": CORAA_REPO,
                        "source_revision": CORAA_REVISION,
                        "source_fixture_id": row["file_path"],
                        "source_subset": row.get("dataset"),
                        "accent": row.get("accent"),
                        "speech_style": row.get("speech_style"),
                        "license": "unknown; do not redistribute selected audio",
                    }
                )
                accepted = True
            if candidates:
                active.append(group)

    if len(selected) != clip_count:
        raise RuntimeError(f"selected only {len(selected)} CORAA clips")
    return selected


def raw_audio_bytes(audio: dict) -> bytes:
    if audio.get("bytes") is not None:
        return audio["bytes"]
    path = audio.get("path")
    if not path:
        raise RuntimeError("CAMOES audio row has neither bytes nor path")
    return Path(path).read_bytes()


def prepare_camoes(out_dir: Path, hf_cache: Path, clip_count: int) -> list[dict]:
    prepared_dir = (
        hf_cache
        / "inesc-id___camoes_si"
        / "default"
        / "0.0.0"
        / CAMOES_REVISION
    )
    arrow_shards = sorted(prepared_dir.glob("camoes_si-test-*.arrow"))
    if arrow_shards:
        dataset = concatenate_datasets(
            [Dataset.from_file(str(shard)) for shard in arrow_shards]
        )
    else:
        dataset = load_dataset(
            CAMOES_REPO,
            revision=CAMOES_REVISION,
            split="test",
            cache_dir=str(hf_cache),
        )
    dataset = dataset.cast_column("audio", Audio(decode=False))
    ordered = sorted(
        range(len(dataset)),
        key=lambda index: stable_key(str(dataset[index].get("ID", index))),
    )
    selected: list[dict] = []
    used_speakers: set[str] = set()
    deferred: list[int] = []

    for index in ordered:
        row = dataset[index]
        speaker = str(row.get("speaker_id") or "")
        if speaker and speaker in used_speakers:
            deferred.append(index)
            continue
        if add_camoes_row(row, index, out_dir, selected):
            if speaker:
                used_speakers.add(speaker)
        if len(selected) == clip_count:
            break

    if len(selected) < clip_count:
        for index in deferred:
            if add_camoes_row(dataset[index], index, out_dir, selected):
                if len(selected) == clip_count:
                    break

    if len(selected) != clip_count:
        raise RuntimeError(f"selected only {len(selected)} CAMOES clips")
    return selected


def add_camoes_row(row: dict, index: int, out_dir: Path, selected: list[dict]) -> bool:
    raw = raw_audio_bytes(row["audio"])
    try:
        info = sf.info(io.BytesIO(raw))
    except RuntimeError:
        return False
    duration = info.frames / info.samplerate
    if not MIN_DURATION_S <= duration <= MAX_DURATION_S:
        return False
    samples, sample_rate = sf.read(io.BytesIO(raw), dtype="float32", always_2d=False)
    source_id = str(row.get("ID") or index)
    fixture_id = safe_id("ptpt", source_id)
    audio_path = out_dir / f"{fixture_id}.wav"
    sf.write(audio_path, samples, sample_rate, subtype="PCM_16")
    selected.append(
        {
            "id": fixture_id,
            "dialect": "pt-PT",
            "audio": audio_path.name,
            "reference": str(row["reference"]).strip(),
            "duration_s": round(duration, 4),
            "speaker_id": row.get("speaker_id"),
            "source_dataset": CAMOES_REPO,
            "source_revision": CAMOES_REVISION,
            "source_fixture_id": source_id,
            "source_subset": row.get("dataset"),
            "license": "CC-BY-NC-4.0",
        }
    )
    return True


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out-dir", default="/tmp/aurum-portuguese-eval")
    parser.add_argument("--hf-cache", default="/tmp/aurum-portuguese-eval/hf-cache")
    parser.add_argument(
        "--clips-per-dialect",
        type=int,
        default=DEFAULT_CLIPS_PER_DIALECT,
        help="balanced clip count for each dialect (default: 10)",
    )
    args = parser.parse_args()
    if args.clips_per_dialect < 1:
        raise SystemExit("--clips-per-dialect must be at least 1")

    out_dir = Path(args.out_dir).resolve()
    hf_cache = Path(args.hf_cache).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    hf_cache.mkdir(parents=True, exist_ok=True)

    items = prepare_coraa(out_dir, hf_cache, args.clips_per_dialect) + prepare_camoes(
        out_dir, hf_cache, args.clips_per_dialect
    )
    manifest = {
        "schema_version": 1,
        "language": "pt",
        "selection": {
            "clips_per_dialect": args.clips_per_dialect,
            "minimum_duration_s": MIN_DURATION_S,
            "maximum_duration_s": MAX_DURATION_S,
            "ordering": "sha256(source fixture id), round-robin CORAA source/accent",
        },
        "caveats": [
            "CORAA's repository declares its license as unknown; audio is local-only.",
            "CAMOES is an in-domain evaluation for the INESC model and is CC-BY-NC-4.0.",
        ],
        "items": items,
    }
    manifest_path = out_dir / "portuguese-stt-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    print(f"WROTE {manifest_path} ({len(items)} clips)")


if __name__ == "__main__":
    main()
