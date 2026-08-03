#!/usr/bin/env python3
"""Run local TTS objective matrix (JOE-2319 / JOE-2217).

Synthesizes eval-pack utterances with aurum CLI, scores mono PCM for safety
metrics (clip, empty, peak, duration), and writes retained reports under
evals/reports/listening/.

This is **objective** evidence only. Three-listener blinded study is separate
(prepare_tts_listening_session.py).

Example:
  AURUM_BIN=aurum python3 scripts/eval/run_tts_objective_matrix.py \\
    --models kitten-nano-int8:Luna,kitten-nano-int8:Jasper,kitten-nano-int8:Bella
"""
from __future__ import annotations

import argparse
import json
import os
import platform
import struct
import subprocess
import sys
import time
import wave
from pathlib import Path


def load_pcm(path: Path) -> tuple[list[float], int, int]:
    with wave.open(str(path), "rb") as w:
        nch, sw, sr, nframes, _, _ = w.getparams()
        raw = w.readframes(nframes)
    if sw != 2:
        raise ValueError(f"expected 16-bit PCM in {path}, sampwidth={sw}")
    count = len(raw) // 2
    ints = struct.unpack("<" + "h" * count, raw)
    if nch > 1:
        # downmix first channel for scoring
        mono = [ints[i] / 32768.0 for i in range(0, count, nch)]
    else:
        mono = [x / 32768.0 for x in ints]
    return mono, sr, 1


def score_pcm(
    fixture: dict,
    samples: list[float],
    sr: int,
    wall_ms: float,
    *,
    max_peak: float = 1.0,
    clip_threshold: float = 0.999,
    max_clipped: int = 0,
    min_duration_ms: int = 50,
) -> dict:
    failures: list[str] = []
    if sr <= 0:
        failures.append("invalid_sample_rate")
    n = len(samples)
    duration_ms = int(n * 1000 / sr) if sr else 0
    peak = 0.0
    sum_sq = 0.0
    clipped = 0
    for s in samples:
        a = abs(s)
        if a > peak:
            peak = a
        sum_sq += s * s
        if a >= clip_threshold:
            clipped += 1
    rms = (sum_sq / n) ** 0.5 if n else 0.0

    # edge silence
    thresh = 1e-4
    lead = 0
    for s in samples:
        if abs(s) < thresh:
            lead += 1
        else:
            break
    trail = 0
    for s in reversed(samples):
        if abs(s) < thresh:
            trail += 1
        else:
            break
    lead_ms = int(lead * 1000 / sr) if sr else 0
    trail_ms = int(trail * 1000 / sr) if sr else 0

    tags = [t.lower() for t in fixture.get("tags") or []]
    is_control = "control" in tags or "invalid_input" in tags
    empty = n < 16 or duration_ms < min_duration_ms
    if not is_control and empty:
        failures.append("empty_or_near_empty")
    if peak > max_peak + 1e-9:
        failures.append(f"peak_exceeds_{max_peak}")
    if clipped > max_clipped:
        failures.append(f"clipped_samples_{clipped}")

    # Invalid-input controls may legitimately produce short/empty audio.
    if is_control and empty:
        # pass if we did not clip
        failures = [f for f in failures if f != "empty_or_near_empty"]

    rtf = (wall_ms / duration_ms) if duration_ms > 0 else None
    return {
        "fixture_id": fixture["id"],
        "sample_rate_hz": sr,
        "channels": 1,
        "sample_count": n,
        "duration_ms": duration_ms,
        "wall_ms": round(wall_ms, 1),
        "rtf": round(rtf, 4) if rtf is not None else None,
        "char_count": len(fixture.get("text") or ""),
        "peak_amplitude": round(peak, 4),
        "rms": round(rms, 4),
        "clipped_samples": clipped,
        "leading_silence_ms": lead_ms,
        "trailing_silence_ms": trail_ms,
        "empty_or_near_empty": empty,
        "truncated": False,
        "passed": len(failures) == 0,
        "failures": failures,
        "tags": fixture.get("tags") or [],
    }


def synth(
    aurum: str,
    text: str,
    *,
    model: str,
    voice: str,
    out_wav: Path,
    local_only: bool,
) -> tuple[int, float, str]:
    out_wav.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        aurum,
        "tts",
        text,
        "--model",
        model,
        "--voice",
        voice,
        "--output-file",
        str(out_wav),
        "--force",
    ]
    if local_only:
        cmd.append("--local-only")
    t0 = time.time()
    r = subprocess.run(cmd, capture_output=True, text=True)
    wall_ms = (time.time() - t0) * 1000.0
    err = (r.stderr or "") + (r.stdout or "")
    return r.returncode, wall_ms, err[-500:]


def parse_pairs(s: str) -> list[tuple[str, str]]:
    out = []
    for part in s.split(","):
        part = part.strip()
        if not part:
            continue
        if ":" not in part:
            raise SystemExit(f"model:voice pair required, got {part!r}")
        m, v = part.split(":", 1)
        out.append((m.strip(), v.strip()))
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pack", default="evals/observatory/tts_eval_pack.v1.json")
    ap.add_argument(
        "--models",
        default="kitten-nano-int8:Luna,kitten-nano-int8:Jasper,kitten-nano-int8:Bella",
        help="Comma-separated model:voice pairs",
    )
    ap.add_argument("--aurum", default=os.environ.get("AURUM_BIN", "aurum"))
    ap.add_argument("--out-dir", default="evals/reports/listening")
    ap.add_argument(
        "--audio-dir",
        default="evals/reports/_local/tts_objective_audio",
        help="Scratch WAV output (gitignored under _local)",
    )
    ap.add_argument("--profile", default="apple_silicon_metal")
    ap.add_argument(
        "--max-fixtures",
        type=int,
        default=0,
        help="0 = all pack fixtures; positive caps for smoke",
    )
    ap.add_argument(
        "--participation",
        default="both,objective_only",
        help="Comma-separated participation filters",
    )
    ap.add_argument("--local-only", action="store_true", help="Fail if pack not cached")
    args = ap.parse_args()

    root = Path.cwd()
    pack_path = root / args.pack
    pack = json.loads(pack_path.read_text(encoding="utf-8"))
    fixtures = list(pack.get("fixtures") or [])
    want_part = {p.strip() for p in args.participation.split(",") if p.strip()}
    if want_part:
        fixtures = [
            f
            for f in fixtures
            if (f.get("participation") or "both") in want_part
            or f.get("participation") is None
        ]
    # Skip invalid_input controls that are expected to error at synth time
    # still include them but handle synth failures as expected for control tags.
    if args.max_fixtures > 0:
        fixtures = fixtures[: args.max_fixtures]

    pairs = parse_pairs(args.models)
    out_dir = root / args.out_dir
    out_dir.mkdir(parents=True, exist_ok=True)
    audio_root = root / args.audio_dir
    audio_root.mkdir(parents=True, exist_ok=True)

    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    version = (root / "VERSION").read_text(encoding="utf-8").strip()
    aurum = args.aurum
    if not Path(aurum).is_file() and not __import__("shutil").which(aurum):
        raise SystemExit(f"aurum binary not found: {aurum}")

    matrix_summary = []
    for model, voice in pairs:
        print(f"=== {model} / {voice} ({len(fixtures)} fixtures) ===")
        scores = []
        for fix in fixtures:
            fid = fix["id"]
            text = fix.get("text") or ""
            tags = [t.lower() for t in fix.get("tags") or []]
            is_invalid = "invalid_input" in tags
            wav = audio_root / model / voice / f"{fid}.wav"
            code, wall_ms, err = synth(
                aurum, text, model=model, voice=voice, out_wav=wav, local_only=args.local_only
            )
            if code != 0 or not wav.is_file():
                if is_invalid:
                    scores.append(
                        {
                            "fixture_id": fid,
                            "passed": True,
                            "failures": [],
                            "notes": "invalid_input control rejected or empty as expected",
                            "wall_ms": round(wall_ms, 1),
                            "tags": fix.get("tags") or [],
                            "synth_failed": True,
                        }
                    )
                    print(f"  {fid}: control synth fail (expected ok)")
                    continue
                scores.append(
                    {
                        "fixture_id": fid,
                        "passed": False,
                        "failures": ["synth_failed"],
                        "wall_ms": round(wall_ms, 1),
                        "tags": fix.get("tags") or [],
                        "error_tail": err,
                    }
                )
                print(f"  FAIL {fid}: synth")
                continue
            samples, sr, _ = load_pcm(wav)
            sc = score_pcm(fix, samples, sr, wall_ms)
            scores.append(sc)
            flag = "OK" if sc["passed"] else "FAIL"
            print(
                f"  {flag} {fid}: dur={sc['duration_ms']}ms peak={sc['peak_amplitude']} "
                f"clip={sc['clipped_samples']} rtf={sc.get('rtf')}"
            )

        passed = sum(1 for s in scores if s.get("passed"))
        failed = len(scores) - passed
        report = {
            "schema_version": 1,
            "evidence_version": "0.0.22-tts-listening-v1",
            "kind": "tts_objective",
            "pack_name": pack.get("name"),
            "pack_version": pack.get("pack_version"),
            "model": model,
            "voice": voice,
            "hardware_profile": args.profile,
            "host": "maintainer-profile-host",
            "os": platform.platform(),
            "machine": platform.machine(),
            "aurum_version": version,
            "commit": commit,
            "fixture_count": len(scores),
            "passed_count": passed,
            "failed_count": failed,
            "all_passed": failed == 0,
            "notes": (
                "Objective PCM safety/correctness matrix (JOE-2319). "
                "Not a three-listener blinded study."
            ),
            "scores": scores,
        }
        out = out_dir / f"tts-objective-{args.profile}-{model}-{voice}.json"
        out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        md = out.with_suffix(".md")
        lines = [
            f"# TTS objective — `{model}` / `{voice}`",
            "",
            f"- Evidence: `{report['evidence_version']}`",
            f"- Pack: {report['pack_name']} ({report['pack_version']})",
            f"- Passed: **{passed}/{len(scores)}**",
            f"- Profile: {args.profile}",
            "",
            "| Fixture | dur_ms | peak | clip | pass |",
            "|---------|--------|------|------|------|",
        ]
        for s in scores:
            lines.append(
                f"| {s.get('fixture_id')} | {s.get('duration_ms', '')} | "
                f"{s.get('peak_amplitude', '')} | {s.get('clipped_samples', '')} | "
                f"{'yes' if s.get('passed') else 'no'} |"
            )
        lines.append("")
        md.write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"WROTE {out} all_passed={report['all_passed']}")
        matrix_summary.append(
            {
                "model": model,
                "voice": voice,
                "passed": passed,
                "failed": failed,
                "all_passed": report["all_passed"],
                "report": str(out.relative_to(root)),
            }
        )

    summary = {
        "schema_version": 1,
        "evidence_version": "0.0.22-tts-listening-v1",
        "kind": "tts_objective_matrix_summary",
        "hardware_profile": args.profile,
        "aurum_version": version,
        "commit": commit,
        "pairs": matrix_summary,
        "all_pairs_passed": all(p["all_passed"] for p in matrix_summary),
        "notes": "Summary of per-voice objective reports. Human listening is separate.",
    }
    sum_path = out_dir / f"tts-objective-matrix-{args.profile}.json"
    sum_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(f"WROTE {sum_path} all_pairs_passed={summary['all_pairs_passed']}")


if __name__ == "__main__":
    main()
