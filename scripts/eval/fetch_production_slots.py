#!/usr/bin/env python3
"""Fetch / assemble STT production-pack slots (JOE-2318).

Operator-machine helper. Audio stays under evals/observatory/cache/ (gitignored).
Never vendors private Plaud/user recordings. Never re-uploads NC corpora as CI
artifacts.

Supported automated slots:
  * librispeech_clean_subset  — OpenSLR-12 test-clean (CC BY 4.0)
  * controls_silence_nonspeech — copy in-repo silence/tone controls
  * musan_noise_mix           — ffmpeg noise overlays on LS clean speech
                                 (no full MUSAN download; disk-friendly)
  * long_form_assemblies      — ≥3 concatenations of LS chapters (>10 min)
  * common_voice_accents      — best-effort HuggingFace Common Voice stream
                                 (optional; skipped cleanly if unavailable)

Usage examples:
  python3 scripts/eval/fetch_production_slots.py --cache-dir evals/observatory/cache \\
      fetch librispeech_clean_subset
  python3 scripts/eval/fetch_production_slots.py --cache-dir evals/observatory/cache \\
      assemble
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from collections import defaultdict
from pathlib import Path
from typing import Any

# OpenSLR-12 LibriSpeech test-clean (public, CC BY 4.0).
LIBRISPEECH_TEST_CLEAN_URL = (
    "https://www.openslr.org/resources/12/test-clean.tar.gz"
)
# Mirror (same content; used if primary fails).
LIBRISPEECH_TEST_CLEAN_MIRROR = (
    "https://us.openslr.org/resources/12/test-clean.tar.gz"
)

# Target: slot min_duration_secs=900; production pack wants ≥3600 overall.
# test-clean is ~5.4 h / 40 speakers — keep the full partition when disk allows.
LS_MAX_UTTERANCES: int | None = None  # None = all
LS_MIN_UTT_SECS = 1.0


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def write_sha256sums(slot_dir: Path, paths: list[Path], *, known: dict[Path, str] | None = None) -> Path:
    """Write SHA256SUMS for files under slot_dir. Paths outside slot_dir are skipped."""
    known = known or {}
    lines = []
    for p in sorted({q.resolve() for q in paths if q.is_file()}):
        try:
            rel = p.relative_to(slot_dir.resolve())
        except ValueError:
            continue
        digest = known.get(p) or known.get(Path(p)) or sha256_file(p)
        lines.append(f"{digest}  {rel.as_posix()}")
    out = slot_dir / "SHA256SUMS"
    out.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")
    return out


def download(url: str, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.is_file() and dest.stat().st_size > 1_000_000:
        print(f"  reuse existing download: {dest} ({dest.stat().st_size} bytes)")
        return
    tmp = dest.with_suffix(dest.suffix + ".partial")
    print(f"  downloading {url}")
    print(f"  -> {dest}")
    try:
        urllib.request.urlretrieve(url, tmp)  # noqa: S310 — fixed OpenSLR URL
        tmp.replace(dest)
    except Exception as e:
        if tmp.is_file():
            tmp.unlink(missing_ok=True)
        raise SystemExit(f"download failed: {e}") from e


def flac_duration_secs(path: Path) -> float:
    """Duration via ffprobe (required for production assemble)."""
    try:
        out = subprocess.check_output(
            [
                "ffprobe",
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                str(path),
            ],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
        return float(out)
    except (subprocess.CalledProcessError, ValueError, FileNotFoundError):
        # LibriSpeech FLAC is 16 kHz mono PCM; approximate from size if needed.
        # 16-bit mono 16 kHz ≈ 32000 bytes/s + small header.
        return max(0.0, (path.stat().st_size - 128) / 32000.0)


def fetch_librispeech(cache: Path) -> dict[str, Any]:
    slot = cache / "librispeech_clean_subset"
    slot.mkdir(parents=True, exist_ok=True)
    tarball = cache / "downloads" / "test-clean.tar.gz"
    try:
        download(LIBRISPEECH_TEST_CLEAN_URL, tarball)
    except SystemExit:
        print("  primary OpenSLR failed; trying us.openslr.org mirror")
        download(LIBRISPEECH_TEST_CLEAN_MIRROR, tarball)

    extract_root = slot / "LibriSpeech"
    if not (extract_root / "test-clean").is_dir():
        print(f"  extracting {tarball} -> {slot}")
        with tarfile.open(tarball, "r:gz") as tf:
            # Python 3.12+ filter= for path safety
            try:
                tf.extractall(slot, filter="data")  # type: ignore[call-arg]
            except TypeError:
                tf.extractall(slot)
    else:
        print(f"  reuse extracted tree: {extract_root / 'test-clean'}")

    fixtures: list[dict[str, Any]] = []
    audio_paths: list[Path] = []
    speakers: set[str] = set()
    total_dur = 0.0
    utt_count = 0

    test_clean = extract_root / "test-clean"
    if not test_clean.is_dir():
        raise SystemExit(f"expected LibriSpeech/test-clean under {slot}")

    for speaker_dir in sorted(p for p in test_clean.iterdir() if p.is_dir()):
        speaker_id = speaker_dir.name
        for chapter_dir in sorted(p for p in speaker_dir.iterdir() if p.is_dir()):
            # Transcript file: <spk>-<chap>.trans.txt
            trans_files = list(chapter_dir.glob("*.trans.txt"))
            if not trans_files:
                continue
            refs: dict[str, str] = {}
            for tf in trans_files:
                for line in tf.read_text(encoding="utf-8", errors="replace").splitlines():
                    line = line.strip()
                    if not line:
                        continue
                    parts = line.split(maxsplit=1)
                    if len(parts) == 2:
                        refs[parts[0]] = parts[1]
                    elif len(parts) == 1:
                        refs[parts[0]] = ""
            for flac in sorted(chapter_dir.glob("*.flac")):
                utt_id = flac.stem  # e.g. 1089-134686-0000
                if utt_id not in refs:
                    continue
                if LS_MAX_UTTERANCES is not None and utt_count >= LS_MAX_UTTERANCES:
                    break
                dur = flac_duration_secs(flac)
                if dur < LS_MIN_UTT_SECS:
                    continue
                rel = flac.relative_to(cache).as_posix()
                fixtures.append(
                    {
                        "id": f"ls_tc_{utt_id}",
                        "audio": rel,
                        # SHA deferred to score/lockfile sample (full-partition hash is slow).
                        "duration_secs": round(dur, 3),
                        "language": "en",
                        "reference": refs[utt_id].lower(),
                        "normalization_policy": "normalize_v1_lower_alnum_ws",
                        "tags": [
                            "clean",
                            "conversational",
                            "read_speech",
                            "accent_us",
                            "librispeech",
                            "numbers" if any(ch.isdigit() for ch in refs[utt_id]) else "clean",
                        ],
                        "speaker_id": f"ls_{speaker_id}",
                        "license": "CC BY 4.0 (LibriSpeech / LibriVox)",
                        "provenance": (
                            "OpenSLR-12 test-clean; "
                            f"https://www.openslr.org/12/; utt={utt_id}"
                        ),
                        "asset_resolution": "external_fetch",
                        "redistributable": False,
                        "timestamps_expected_reliable": True,
                    }
                )
                audio_paths.append(flac)
                speakers.add(speaker_id)
                total_dur += dur
                utt_count += 1
            if LS_MAX_UTTERANCES is not None and utt_count >= LS_MAX_UTTERANCES:
                break
        if LS_MAX_UTTERANCES is not None and utt_count >= LS_MAX_UTTERANCES:
            break

    # Drop duplicate tag "clean" when numbers path also set clean
    for f in fixtures:
        tags = f["tags"]
        f["tags"] = list(dict.fromkeys(tags))

    meta = {
        "slot_id": "librispeech_clean_subset",
        "source_url": LIBRISPEECH_TEST_CLEAN_URL,
        "tarball_sha256": sha256_file(tarball),
        "fixture_count": len(fixtures),
        "speaker_count": len(speakers),
        "total_duration_secs": round(total_dur, 1),
        "license_family": "CC BY 4.0 (LibriSpeech)",
    }
    (slot / "fixtures.json").write_text(
        json.dumps({"schema_version": 1, "fixtures": fixtures, "meta": meta}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    # Compact lockfile: first 50 utterances only (full partition is large).
    sample_paths = audio_paths[:50]
    write_sha256sums(slot, sample_paths)
    (slot / "SOURCE.txt").write_text(
        f"url={LIBRISPEECH_TEST_CLEAN_URL}\n"
        f"tarball={tarball}\n"
        f"tarball_sha256={meta['tarball_sha256']}\n"
        f"fixtures={len(fixtures)}\n"
        f"speakers={len(speakers)}\n"
        f"duration_secs={meta['total_duration_secs']}\n",
        encoding="utf-8",
    )
    print(
        f"OK librispeech_clean_subset: {len(fixtures)} utts, "
        f"{len(speakers)} speakers, {total_dur/60:.1f} min"
    )
    return meta


def fetch_controls(cache: Path, repo_root: Path) -> dict[str, Any]:
    slot = cache / "controls_silence_nonspeech"
    slot.mkdir(parents=True, exist_ok=True)
    audio_src = repo_root / "evals" / "audio"
    mapping = [
        ("silence_1s.wav", "core_silence", "", ["silence", "control"]),
        ("tone_440_1s.wav", "core_non_speech_tone", "", ["non_speech", "control"]),
    ]
    fixtures = []
    paths = []
    for name, fid, ref, tags in mapping:
        src = audio_src / name
        if not src.is_file():
            print(f"  WARN missing {src}", file=sys.stderr)
            continue
        dest = slot / name
        shutil.copy2(src, dest)
        paths.append(dest)
        rel = dest.relative_to(cache).as_posix()
        fixtures.append(
            {
                "id": f"prod_{fid}",
                "audio": rel,
                "audio_sha256": sha256_file(dest),
                "duration_secs": 1.0,
                "language": "en",
                "reference": ref,
                "normalization_policy": "normalize_v1_lower_alnum_ws",
                "tags": tags,
                "speaker_id": None,
                "license": "synthetic CC0",
                "provenance": "aurum evals/audio (in-repo)",
                "asset_resolution": "redistributable",
                "redistributable": True,
                "timestamps_expected_reliable": True,
            }
        )
    meta = {
        "slot_id": "controls_silence_nonspeech",
        "fixture_count": len(fixtures),
        "total_duration_secs": float(len(fixtures)),
    }
    (slot / "fixtures.json").write_text(
        json.dumps({"schema_version": 1, "fixtures": fixtures, "meta": meta}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    write_sha256sums(slot, paths)
    print(f"OK controls_silence_nonspeech: {len(fixtures)} fixtures")
    return meta


def _load_ls_fixtures(cache: Path) -> list[dict[str, Any]]:
    p = cache / "librispeech_clean_subset" / "fixtures.json"
    if not p.is_file():
        raise SystemExit(
            "librispeech_clean_subset fixtures missing — run fetch librispeech_clean_subset first"
        )
    return json.loads(p.read_text(encoding="utf-8"))["fixtures"]


def fetch_noise_mix(cache: Path) -> dict[str, Any]:
    """Disk-friendly noisy overlays (ffmpeg anoisesrc) on clean LS utterances.

    Full MUSAN is multi-GB; this slot documents the role and produces real noisy
    speech without re-hosting MUSAN. Provenance notes the synthetic noise source.
    """
    if not shutil.which("ffmpeg"):
        raise SystemExit("ffmpeg required for musan_noise_mix")
    slot = cache / "musan_noise_mix"
    slot.mkdir(parents=True, exist_ok=True)
    ls = _load_ls_fixtures(cache)
    # Pick longer clean utterances from distinct speakers.
    by_spk: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for f in ls:
        by_spk[f.get("speaker_id") or "unk"].append(f)
    picks: list[dict[str, Any]] = []
    for spk, items in sorted(by_spk.items()):
        items = sorted(items, key=lambda x: -float(x.get("duration_secs") or 0))
        if items:
            picks.append(items[0])
        if len(picks) >= 24:
            break
    fixtures = []
    paths = []
    total = 0.0
    for i, src_fix in enumerate(picks):
        src = cache / src_fix["audio"]
        if not src.is_file():
            continue
        out = slot / f"noisy_{src_fix['id']}.wav"
        # Mix speech with mild white noise (~0.02 amplitude).
        cmd = [
            "ffmpeg",
            "-y",
            "-i",
            str(src),
            "-filter_complex",
            "[0:a]aformat=sample_rates=16000:channel_layouts=mono[s];"
            "anoisesrc=color=white:amplitude=0.015:sample_rate=16000[n];"
            "[s][n]amix=inputs=2:duration=first:dropout_transition=0[a]",
            "-map",
            "[a]",
            "-c:a",
            "pcm_s16le",
            str(out),
        ]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0 or not out.is_file():
            print(f"  WARN ffmpeg noise mix failed for {src_fix['id']}: {r.stderr[-200:]}", file=sys.stderr)
            continue
        dur = flac_duration_secs(out)
        rel = out.relative_to(cache).as_posix()
        fixtures.append(
            {
                "id": f"noise_{src_fix['id']}",
                "audio": rel,
                "audio_sha256": sha256_file(out),
                "duration_secs": round(dur, 3),
                "language": "en",
                "reference": src_fix["reference"],
                "normalization_policy": "normalize_v1_lower_alnum_ws",
                "tags": ["noisy", "conversational", "accent_us", "noise_mix"],
                "speaker_id": src_fix.get("speaker_id"),
                "license": "CC BY 4.0 speech (LibriSpeech) + synthetic white noise overlay",
                "provenance": (
                    "LibriSpeech clean utt + ffmpeg anoisesrc white noise "
                    "(disk-friendly substitute for full MUSAN download)"
                ),
                "asset_resolution": "external_fetch",
                "redistributable": False,
                "timestamps_expected_reliable": True,
            }
        )
        paths.append(out)
        total += dur
    meta = {
        "slot_id": "musan_noise_mix",
        "fixture_count": len(fixtures),
        "total_duration_secs": round(total, 1),
        "notes": "ffmpeg noise overlay; not full MUSAN corpus",
    }
    (slot / "fixtures.json").write_text(
        json.dumps({"schema_version": 1, "fixtures": fixtures, "meta": meta}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    write_sha256sums(slot, paths)
    print(f"OK musan_noise_mix: {len(fixtures)} fixtures, {total/60:.1f} min")
    return meta


def fetch_long_form(cache: Path) -> dict[str, Any]:
    """Build ≥3 assemblies each >10 min.

    test-clean speakers average ~8 min, so assemblies chain utterances across
    speakers (still licensed LibriSpeech; provenance lists constituent utts).
    """
    if not shutil.which("ffmpeg"):
        raise SystemExit("ffmpeg required for long_form_assemblies")
    slot = cache / "long_form_assemblies"
    slot.mkdir(parents=True, exist_ok=True)
    ls = sorted(_load_ls_fixtures(cache), key=lambda x: x["id"])
    fixtures = []
    paths = []
    target_assemblies = 3
    min_secs = 600.0
    # Non-overlapping windows over the sorted utterance list.
    cursor = 0
    for asm_i in range(target_assemblies):
        selected: list[dict[str, Any]] = []
        dur = 0.0
        while cursor < len(ls) and dur < min_secs:
            selected.append(ls[cursor])
            dur += float(ls[cursor].get("duration_secs") or 0)
            cursor += 1
        if dur < min_secs:
            print(
                f"  WARN not enough remaining audio for assembly {asm_i} "
                f"({dur:.0f}s < {min_secs:.0f}s)",
                file=sys.stderr,
            )
            break
        list_file = slot / f"concat_asm{asm_i:02d}.txt"
        with list_file.open("w", encoding="utf-8") as lf:
            for it in selected:
                p = (cache / it["audio"]).resolve()
                # Escape single quotes for ffmpeg concat demuxer.
                esc = str(p).replace("'", "'\\''")
                lf.write(f"file '{esc}'\n")
        out = slot / f"long_asm{asm_i:02d}.wav"
        cmd = [
            "ffmpeg",
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            str(list_file),
            "-c:a",
            "pcm_s16le",
            "-ar",
            "16000",
            "-ac",
            "1",
            str(out),
        ]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0 or not out.is_file():
            print(
                f"  WARN long-form concat failed asm{asm_i}: {r.stderr[-300:]}",
                file=sys.stderr,
            )
            continue
        real_dur = flac_duration_secs(out)
        ref = " ".join(it["reference"] for it in selected)
        # Cap stored reference size for long assemblies (WER on full hour is rare).
        if len(ref) > 200_000:
            ref = ref[:200_000] + " …"
        rel = out.relative_to(cache).as_posix()
        primary_spk = selected[0].get("speaker_id") or f"asm{asm_i}"
        fixtures.append(
            {
                "id": f"longform_asm{asm_i:02d}",
                "audio": rel,
                "audio_sha256": sha256_file(out),
                "duration_secs": round(real_dur, 3),
                "language": "en",
                "reference": ref,
                "normalization_policy": "normalize_v1_lower_alnum_ws",
                "tags": ["long_form", "lecture", "read_speech", "accent_us", "conversational"],
                "speaker_id": f"longform_primary_{primary_spk}",
                "license": "CC BY 4.0 (LibriSpeech multi-utt assembly)",
                "provenance": (
                    f"concat of {len(selected)} LibriSpeech test-clean utts "
                    f"({selected[0]['id']}…{selected[-1]['id']})"
                ),
                "asset_resolution": "external_fetch",
                "redistributable": False,
                "timestamps_expected_reliable": True,
            }
        )
        paths.append(out)
        print(f"  long-form asm{asm_i:02d}: {real_dur/60:.1f} min ({len(selected)} utts)")

    meta = {
        "slot_id": "long_form_assemblies",
        "fixture_count": len(fixtures),
        "total_duration_secs": round(
            sum(float(f["duration_secs"]) for f in fixtures), 1
        ),
    }
    (slot / "fixtures.json").write_text(
        json.dumps({"schema_version": 1, "fixtures": fixtures, "meta": meta}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    write_sha256sums(slot, paths)
    print(f"OK long_form_assemblies: {len(fixtures)} fixtures")
    return meta


def _map_cv_accent(raw: str) -> str | None:
    """Map Common Voice free-text accent to observatory accent_* tag."""
    a = (raw or "").strip().lower()
    if not a:
        return None
    # Prefer primary label before commas.
    primary = a.split(",")[0].strip()
    rules = [
        ("united states", "accent_us"),
        ("england english", "accent_uk"),
        ("scottish", "accent_sc"),
        ("irish", "accent_ie"),
        ("australian", "accent_au"),
        ("new zealand", "accent_nz"),
        ("canadian", "accent_ca"),
        ("india and south asia", "accent_in"),
        ("southern african", "accent_za"),
        ("hong kong", "accent_hk"),
        ("filipino", "accent_ph"),
        ("singapore", "accent_sg"),
        ("welsh", "accent_wls"),
    ]
    for key, tag in rules:
        if key in primary or key in a:
            return tag
    return None


def fetch_common_voice_accents(cache: Path) -> dict[str, Any]:
    """Multi-accent English via fsicoli/common_voice_17_0 (CC0 mirror on HF).

    Downloads en/dev TSV + en_dev audio tar, extracts only selected accent clips,
    then removes the tar to save disk. Requires `huggingface_hub` and ffmpeg.
    """
    slot = cache / "common_voice_accents"
    slot.mkdir(parents=True, exist_ok=True)
    try:
        from huggingface_hub import hf_hub_download  # type: ignore
    except ImportError:
        print(
            "SKIP common_voice_accents: pip install huggingface_hub",
            file=sys.stderr,
        )
        meta = {
            "slot_id": "common_voice_accents",
            "skipped": True,
            "reason": "huggingface_hub not installed",
            "fixture_count": 0,
            "total_duration_secs": 0.0,
        }
        (slot / "fixtures.json").write_text(
            json.dumps({"schema_version": 1, "fixtures": [], "meta": meta}, indent=2)
            + "\n",
            encoding="utf-8",
        )
        return meta

    import csv

    repo = "fsicoli/common_voice_17_0"
    per_accent = 12
    # Aim for ≥4 distinct accent tags with real speech.
    wanted_tags = {"accent_us", "accent_uk", "accent_in", "accent_au", "accent_ca", "accent_za"}

    print(f"  downloading {repo} transcript/en/dev.tsv …")
    try:
        tsv_path = Path(
            hf_hub_download(repo, "transcript/en/dev.tsv", repo_type="dataset")
        )
    except Exception as e:
        print(f"SKIP common_voice_accents: tsv download failed: {e}", file=sys.stderr)
        meta = {
            "slot_id": "common_voice_accents",
            "skipped": True,
            "reason": str(e),
            "fixture_count": 0,
            "total_duration_secs": 0.0,
        }
        (slot / "fixtures.json").write_text(
            json.dumps({"schema_version": 1, "fixtures": [], "meta": meta}, indent=2)
            + "\n",
            encoding="utf-8",
        )
        return meta

    selected_by_tag: dict[str, list[dict[str, str]]] = defaultdict(list)
    with tsv_path.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f, delimiter="\t")
        for row in reader:
            tag = _map_cv_accent(row.get("accents") or "")
            if tag is None or tag not in wanted_tags:
                continue
            if len(selected_by_tag[tag]) >= per_accent:
                continue
            path = (row.get("path") or "").strip()
            sentence = (row.get("sentence") or "").strip()
            client = (row.get("client_id") or "anon")[:16]
            if not path or not sentence:
                continue
            selected_by_tag[tag].append(
                {
                    "path": path,
                    "sentence": sentence,
                    "client_id": client,
                    "accent_raw": row.get("accents") or "",
                }
            )
            filled = sum(1 for v in selected_by_tag.values() if len(v) >= per_accent)
            if filled >= 4:
                break

    needed_names = {r["path"] for rows in selected_by_tag.values() for r in rows}
    if len(selected_by_tag) < 4:
        print(
            f"  WARN only {len(selected_by_tag)} accent buckets in dev.tsv "
            f"({list(selected_by_tag)})",
            file=sys.stderr,
        )

    print(
        f"  selected {len(needed_names)} clips across accents "
        f"{ {k: len(v) for k, v in selected_by_tag.items()} }"
    )
    print(f"  downloading {repo} audio/en/dev/en_dev_0.tar (~700MB) …")
    try:
        tar_path = Path(
            hf_hub_download(repo, "audio/en/dev/en_dev_0.tar", repo_type="dataset")
        )
    except Exception as e:
        print(f"SKIP common_voice_accents: audio tar failed: {e}", file=sys.stderr)
        meta = {
            "slot_id": "common_voice_accents",
            "skipped": True,
            "reason": str(e),
            "fixture_count": 0,
            "total_duration_secs": 0.0,
        }
        (slot / "fixtures.json").write_text(
            json.dumps({"schema_version": 1, "fixtures": [], "meta": meta}, indent=2)
            + "\n",
            encoding="utf-8",
        )
        return meta

    raw_dir = slot / "raw_mp3"
    raw_dir.mkdir(parents=True, exist_ok=True)
    extracted = 0
    print(f"  extracting selected mp3s from {tar_path.name} …")
    with tarfile.open(tar_path, "r:") as tf:
        # Member names may include subdirs; match on basename.
        members_by_base = {}
        for m in tf.getmembers():
            if not m.isfile():
                continue
            base = Path(m.name).name
            members_by_base[base] = m
        for name in needed_names:
            m = members_by_base.get(name) or members_by_base.get(Path(name).name)
            if m is None:
                continue
            dest = raw_dir / Path(m.name).name
            if dest.is_file() and dest.stat().st_size > 0:
                extracted += 1
                continue
            src = tf.extractfile(m)
            if src is None:
                continue
            dest.write_bytes(src.read())
            extracted += 1
    print(f"  extracted {extracted}/{len(needed_names)} mp3 files")

    fixtures: list[dict[str, Any]] = []
    paths: list[Path] = []
    total = 0.0
    collected_counts: dict[str, int] = defaultdict(int)
    for tag, rows in selected_by_tag.items():
        for i, row in enumerate(rows):
            mp3 = raw_dir / Path(row["path"]).name
            if not mp3.is_file():
                continue
            wav = slot / f"cv_{tag}_{i:03d}.wav"
            if not wav.is_file():
                cmd = [
                    "ffmpeg",
                    "-y",
                    "-i",
                    str(mp3),
                    "-ac",
                    "1",
                    "-ar",
                    "16000",
                    "-c:a",
                    "pcm_s16le",
                    str(wav),
                ]
                r = subprocess.run(cmd, capture_output=True, text=True)
                if r.returncode != 0 or not wav.is_file():
                    print(f"  WARN ffmpeg convert failed {mp3.name}", file=sys.stderr)
                    continue
            dur = flac_duration_secs(wav)
            rel = wav.relative_to(cache).as_posix()
            fixtures.append(
                {
                    "id": f"cv_{tag}_{i:03d}",
                    "audio": rel,
                    "audio_sha256": sha256_file(wav),
                    "duration_secs": round(dur, 3),
                    "language": "en",
                    "reference": row["sentence"].lower(),
                    "normalization_policy": "normalize_v1_lower_alnum_ws",
                    "tags": ["conversational", "common_voice", tag],
                    "speaker_id": f"cv_{row['client_id']}",
                    "license": "CC0-1.0 (Mozilla Common Voice 17.0)",
                    "provenance": (
                        f"fsicoli/common_voice_17_0 en/dev accent={row['accent_raw']!r} "
                        f"clip={row['path']}"
                    ),
                    "asset_resolution": "external_fetch",
                    "redistributable": False,
                    "timestamps_expected_reliable": True,
                }
            )
            paths.append(wav)
            total += dur
            collected_counts[tag] += 1

    meta = {
        "slot_id": "common_voice_accents",
        "fixture_count": len(fixtures),
        "total_duration_secs": round(total, 1),
        "accents": dict(collected_counts),
        "source_repo": repo,
        "skipped": len(fixtures) == 0,
    }
    (slot / "fixtures.json").write_text(
        json.dumps({"schema_version": 1, "fixtures": fixtures, "meta": meta}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    write_sha256sums(slot, paths)
    (slot / "SOURCE.txt").write_text(
        f"repo={repo}\n"
        f"license=CC0-1.0\n"
        f"split=en/dev\n"
        f"accents={dict(collected_counts)}\n"
        f"fixtures={len(fixtures)}\n",
        encoding="utf-8",
    )
    print(
        f"OK common_voice_accents: {len(fixtures)} fixtures, "
        f"accents={dict(collected_counts)}, {total/60:.1f} min"
    )
    return meta


def assemble_production(cache: Path) -> Path:
    """Merge slot fixtures into cache/corpus.production.json."""
    slot_ids = [
        "librispeech_clean_subset",
        "common_voice_accents",
        "tedlium_lecture",
        "musan_noise_mix",
        "long_form_assemblies",
        "multilingual_codeswitch",
        "controls_silence_nonspeech",
    ]
    fixtures: list[dict[str, Any]] = []
    slot_status: dict[str, Any] = {}
    for sid in slot_ids:
        fp = cache / sid / "fixtures.json"
        if not fp.is_file():
            slot_status[sid] = "missing"
            continue
        data = json.loads(fp.read_text(encoding="utf-8"))
        slot_fx = data.get("fixtures") or []
        meta = data.get("meta") or {}
        if meta.get("skipped") and not slot_fx:
            slot_status[sid] = f"skipped: {meta.get('reason', 'n/a')}"
            continue
        fixtures.extend(slot_fx)
        slot_status[sid] = {
            "fixture_count": len(slot_fx),
            "duration_secs": round(
                sum(float(f.get("duration_secs") or 0) for f in slot_fx), 1
            ),
        }

    # Deduplicate by id
    seen = set()
    uniq = []
    for f in fixtures:
        if f["id"] in seen:
            continue
        seen.add(f["id"])
        uniq.append(f)
    fixtures = uniq

    pack = {
        "schema_version": 1,
        "name": "aurum-observatory-production-v1",
        "corpus_version": "observatory-production-v1",
        "description": (
            "Operator-assembled production STT pack from licensed open sources. "
            "Audio paths are relative to evals/observatory/cache/. "
            "Not redistributable as a whole; do not commit private audio."
        ),
        "dry_run": False,
        "slot_status": slot_status,
        "fixtures": fixtures,
    }
    out = cache / "corpus.production.json"
    out.write_text(json.dumps(pack, indent=2) + "\n", encoding="utf-8")
    total = sum(float(f.get("duration_secs") or 0) for f in fixtures)
    speakers = {f.get("speaker_id") for f in fixtures if f.get("speaker_id")}
    accents = set()
    for f in fixtures:
        for t in f.get("tags") or []:
            if str(t).startswith("accent_"):
                accents.add(t)
    print(
        f"wrote {out}: {len(fixtures)} fixtures, {total/60:.1f} min, "
        f"{len(speakers)} speakers, {len(accents)} accent tags"
    )
    print("slot_status:", json.dumps(slot_status, indent=2))
    return out


def score_subset(
    cache: Path,
    repo_root: Path,
    *,
    aurum: str,
    model: str,
    profile: str,
    max_fixtures: int,
    out_dir: Path,
) -> Path:
    """Run aurum on a capped production subset and write a retained report."""
    pack_path = cache / "corpus.production.json"
    if not pack_path.is_file():
        raise SystemExit("run assemble first")
    pack = json.loads(pack_path.read_text(encoding="utf-8"))
    fixtures = [f for f in pack.get("fixtures") or [] if f.get("audio")]
    # Prefer diversity: controls + clean + noisy + accents; skip multi-hour longform
    controls = [
        f
        for f in fixtures
        if "control" in (f.get("tags") or [])
        or "silence" in (f.get("tags") or [])
        or "non_speech" in (f.get("tags") or [])
    ]
    noisy = [f for f in fixtures if "noisy" in (f.get("tags") or [])]
    accents = [
        f
        for f in fixtures
        if "common_voice" in (f.get("tags") or [])
        and any(str(t).startswith("accent_") for t in (f.get("tags") or []))
    ]
    clean = [
        f
        for f in fixtures
        if "librispeech" in (f.get("tags") or [])
        and "long_form" not in (f.get("tags") or [])
    ]
    # Cap duration per clean fixture for scorepath (~20s max for speed)
    clean_short = [f for f in clean if float(f.get("duration_secs") or 0) <= 20.0]
    selected: list[dict[str, Any]] = []
    selected.extend(controls[:4])
    selected.extend(noisy[:4])
    # Spread accent tags: up to 2 clips per accent, up to 8 CV total.
    per_accent: dict[str, int] = {}
    for f in accents:
        tags = [str(t) for t in (f.get("tags") or []) if str(t).startswith("accent_")]
        tag = tags[0] if tags else "accent_?"
        if per_accent.get(tag, 0) >= 2:
            continue
        per_accent[tag] = per_accent.get(tag, 0) + 1
        selected.append(f)
        if sum(per_accent.values()) >= 8:
            break
    # Speakers spread for clean LS
    seen_spk = set()
    for f in clean_short:
        spk = f.get("speaker_id")
        if spk in seen_spk and len(selected) > 12:
            continue
        seen_spk.add(spk)
        selected.append(f)
        if len(selected) >= max_fixtures:
            break
    selected = selected[:max_fixtures]

    def norm(s: str) -> str:
        import re

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

    aurum_path = Path(aurum)
    if not aurum_path.is_file():
        # try PATH
        which = shutil.which(aurum)
        if not which:
            raise SystemExit(f"aurum binary not found: {aurum}")
        aurum_path = Path(which)

    import platform
    import time

    scores = []
    out_dir.mkdir(parents=True, exist_ok=True)
    commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=str(repo_root), text=True
    ).strip()
    version = (repo_root / "VERSION").read_text(encoding="utf-8").strip()

    print(f"scoring {len(selected)} fixtures with model={model} aurum={aurum_path}")
    for fix in selected:
        audio = cache / fix["audio"]
        if not audio.is_file():
            print(f"  MISSING {audio}")
            continue
        fid = fix["id"]
        out_txt = out_dir / f"_tmp_{model}_{fid}.txt"
        t0 = time.time()
        r = subprocess.run(
            [
                str(aurum_path),
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
            print(f"  FAIL {fid}: {r.stderr[-300:]}")
            scores.append(
                {
                    "fixture_id": fid,
                    "error": "aurum failed",
                    "wer": 1.0,
                    "cer": 1.0,
                    "silence_false_positive": False,
                    "repetition_ratio": 0.0,
                    "tags": fix.get("tags", []),
                    "wall_s": round(elapsed, 3),
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
        # Public retained reports: omit full hypothesis/reference text (privacy policy).
        scores.append(
            {
                "fixture_id": fid,
                "wer": w,
                "cer": round(lev_rate(ref, hyp, "char"), 4),
                "silence_false_positive": sfp,
                "repetition_ratio": round(rep, 4),
                "tags": fix.get("tags", []),
                "audio_sha256": fix.get("audio_sha256") or sha256_file(audio),
                "duration_secs": fix.get("duration_secs"),
                "wall_s": round(elapsed, 3),
                "rtf": round(elapsed / max(float(fix.get("duration_secs") or 1.0), 0.01), 4),
            }
        )
        print(f"  {fid}: wer={w} rtf={scores[-1]['rtf']} wall={elapsed:.1f}s")

    n = max(len(scores), 1)
    speech = [
        s
        for s in scores
        if not any(t in s.get("tags", []) for t in ("silence", "non_speech", "control"))
    ]
    ns = max(len(speech), 1)
    # Scenario buckets for budget compare (priority order; accents only for CV).
    buckets: dict[str, list[float]] = {}
    for s in scores:
        tags = [str(t).lower() for t in (s.get("tags") or [])]
        if "silence" in tags:
            key = "silence"
        elif "non_speech" in tags:
            key = "non_speech"
        elif "noisy" in tags or "noise_mix" in tags:
            key = "noisy"
        elif "common_voice" in tags:
            accents = [t for t in tags if t.startswith("accent_")]
            key = accents[0] if accents else "common_voice"
        elif "librispeech" in tags or "clean" in tags:
            key = "clean"
        elif "long_form" in tags:
            key = "long_form"
        else:
            key = tags[0] if tags else "untagged"
        buckets.setdefault(key, []).append(float(s.get("wer") or 0.0))
    scenario_mean_wer = {
        k: round(sum(v) / len(v), 4) for k, v in sorted(buckets.items()) if v
    }
    report = {
        "schema_version": 1,
        "corpus_version": pack.get("corpus_version"),
        "corpus_name": pack.get("name"),
        "evidence_version": "0.0.22-observatory-v1",
        "kind": "stt_production_subset",
        "model": model,
        "backend_kind": "asr",
        "provider": "local",
        "hardware_profile": profile,
        "host": "maintainer-profile-host",
        "os": platform.platform(),
        "machine": platform.machine(),
        "aurum_version": version,
        "commit": commit,
        "fixture_count_scored": len(scores),
        "notes": (
            "Production-pack subset scorepath (JOE-2318). Real licensed speech "
            "(LibriSpeech, noise overlays, and/or Common Voice accents). "
            "Hypotheses omitted from public report. Capped fixture count — not a full "
            "hour sweep of every production fixture."
        ),
        "scenario_mean_wer": scenario_mean_wer,
        "stt_scores": scores,
        "mean_wer": round(sum(s["wer"] for s in scores) / n, 4),
        "mean_cer": round(sum(s["cer"] for s in scores) / n, 4),
        "mean_wer_speech_only": round(sum(s["wer"] for s in speech) / ns, 4),
        "silence_false_positives": sum(
            1 for s in scores if s.get("silence_false_positive")
        ),
        "mean_repetition_ratio": round(
            sum(s.get("repetition_ratio", 0) for s in scores) / n, 4
        ),
        "mean_rtf": round(sum(s.get("rtf", 0) for s in scores) / n, 4),
    }
    out = out_dir / f"stt-production-subset-{profile}-{model}.json"
    out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        f"WROTE {out} mean_wer={report['mean_wer']} speech_only={report['mean_wer_speech_only']} "
        f"sfp={report['silence_false_positives']} mean_rtf={report['mean_rtf']}"
    )
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--cache-dir",
        default="evals/observatory/cache",
        help="Cache root (default: evals/observatory/cache)",
    )
    ap.add_argument(
        "--repo-root",
        default=None,
        help="Repo root (default: two levels up from this script)",
    )
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_fetch = sub.add_parser("fetch", help="Fetch one or more slots")
    p_fetch.add_argument(
        "slots",
        nargs="+",
        help="Slot ids or 'all-auto' for automated slots",
    )

    sub.add_parser("assemble", help="Assemble corpus.production.json from slot fixtures")

    p_score = sub.add_parser("score-subset", help="Score a capped production subset")
    p_score.add_argument("--model", default="tiny-q5_1")
    p_score.add_argument("--profile", default="apple_silicon_metal")
    p_score.add_argument("--aurum", default=os.environ.get("AURUM_BIN", "aurum"))
    p_score.add_argument("--max-fixtures", type=int, default=24)
    p_score.add_argument("--out-dir", default="evals/reports/stt")

    args = ap.parse_args()
    script = Path(__file__).resolve()
    repo = Path(args.repo_root) if args.repo_root else script.parents[2]
    cache = Path(args.cache_dir)
    if not cache.is_absolute():
        cache = (repo / cache).resolve()
    cache.mkdir(parents=True, exist_ok=True)

    if args.cmd == "fetch":
        slots = args.slots
        if slots == ["all-auto"]:
            slots = [
                "librispeech_clean_subset",
                "controls_silence_nonspeech",
                "musan_noise_mix",
                "long_form_assemblies",
                "common_voice_accents",
            ]
        handlers = {
            "librispeech_clean_subset": lambda: fetch_librispeech(cache),
            "controls_silence_nonspeech": lambda: fetch_controls(cache, repo),
            "musan_noise_mix": lambda: fetch_noise_mix(cache),
            "long_form_assemblies": lambda: fetch_long_form(cache),
            "common_voice_accents": lambda: fetch_common_voice_accents(cache),
        }
        for s in slots:
            if s not in handlers:
                print(
                    f"slot {s!r} has no automated fetcher yet — "
                    f"use prepare_stt_observatory_corpus.sh --slot {s} for the recipe",
                    file=sys.stderr,
                )
                continue
            print(f"=== fetch {s} ===")
            handlers[s]()
    elif args.cmd == "assemble":
        assemble_production(cache)
    elif args.cmd == "score-subset":
        out_dir = Path(args.out_dir)
        if not out_dir.is_absolute():
            out_dir = repo / out_dir
        score_subset(
            cache,
            repo,
            aurum=args.aurum,
            model=args.model,
            profile=args.profile,
            max_fixtures=args.max_fixtures,
            out_dir=out_dir,
        )
    else:
        ap.error("unknown command")


if __name__ == "__main__":
    main()
