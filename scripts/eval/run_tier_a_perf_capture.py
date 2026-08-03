#!/usr/bin/env python3
"""Named-hardware Tier A performance capture (JOE-2317 / JOE-2218).

Captures release-gated workflow scenarios on the current machine with a
versioned PerfReport (schema 2). Hostnames and user paths are redacted.

Requires a cached local STT model (default tiny-q5_1) and optional Kitten TTS.

Examples:
  python3 scripts/eval/run_tier_a_perf_capture.py --profile-id tier_a_macos_arm64
  python3 scripts/eval/run_tier_a_perf_capture.py --profile-id tier_a_linux_x86_64_gnu \\
    --aurum ./target/release/aurum --out evals/reports/perf/perf-tier_a_linux.json
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def percentile(sorted_vals: list[float], p: float) -> float:
    if not sorted_vals:
        return 0.0
    if len(sorted_vals) == 1:
        return sorted_vals[0]
    k = (len(sorted_vals) - 1) * p
    f = int(k)
    c = min(f + 1, len(sorted_vals) - 1)
    if f == c:
        return sorted_vals[f]
    return sorted_vals[f] + (sorted_vals[c] - sorted_vals[f]) * (k - f)


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def detect_hardware(profile_id: str) -> dict:
    system = platform.system()
    machine = platform.machine().lower()
    cores = os.cpu_count() or 0
    mem_gib = 0
    cpu_label = platform.processor() or machine
    os_label = platform.platform()
    tier = "macos_arm64"

    if system == "Darwin":
        tier = "macos_arm64"
        try:
            cpu_label = subprocess.check_output(
                ["sysctl", "-n", "machdep.cpu.brand_string"], text=True
            ).strip()
        except (subprocess.CalledProcessError, FileNotFoundError):
            cpu_label = "Apple Silicon"
        try:
            mem = int(
                subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True).strip()
            )
            mem_gib = int(mem / (1024**3))
        except (subprocess.CalledProcessError, FileNotFoundError, ValueError):
            mem_gib = 0
        try:
            ver = subprocess.check_output(["sw_vers", "-productVersion"], text=True).strip()
            os_label = f"macOS {ver}"
        except (subprocess.CalledProcessError, FileNotFoundError):
            os_label = f"macOS ({platform.mac_ver()[0]})"
    elif system == "Linux":
        tier = "linux_x86_64_gnu" if "x86" in machine or machine == "amd64" else f"linux_{machine}"
        try:
            with open("/proc/cpuinfo", encoding="utf-8", errors="replace") as f:
                for line in f:
                    if line.lower().startswith("model name"):
                        cpu_label = line.split(":", 1)[1].strip()
                        break
        except OSError:
            pass
        try:
            with open("/proc/meminfo", encoding="utf-8") as f:
                for line in f:
                    if line.startswith("MemTotal:"):
                        kb = int(line.split()[1])
                        mem_gib = int(kb / (1024 * 1024))
                        break
        except (OSError, ValueError):
            pass
        try:
            # Prefer /etc/os-release pretty name
            with open("/etc/os-release", encoding="utf-8") as f:
                data = dict(
                    line.strip().split("=", 1)
                    for line in f
                    if "=" in line and not line.strip().startswith("#")
                )
            pretty = data.get("PRETTY_NAME", "").strip('"')
            kernel = platform.release()
            os_label = f"{pretty} kernel {kernel}" if pretty else f"Linux {kernel}"
        except OSError:
            os_label = f"Linux {platform.release()}"
    elif system == "Windows":
        tier = "windows_x86_64_msvc"
        cpu_label = platform.processor() or "x86_64"
        os_label = f"Windows {platform.version()}"
        try:
            import ctypes

            class MEMORYSTATUSEX(ctypes.Structure):
                _fields_ = [
                    ("dwLength", ctypes.c_ulong),
                    ("dwMemoryLoad", ctypes.c_ulong),
                    ("ullTotalPhys", ctypes.c_ulonglong),
                    ("ullAvailPhys", ctypes.c_ulonglong),
                    ("ullTotalPageFile", ctypes.c_ulonglong),
                    ("ullAvailPageFile", ctypes.c_ulonglong),
                    ("ullTotalVirtual", ctypes.c_ulonglong),
                    ("ullAvailVirtual", ctypes.c_ulonglong),
                    ("sullAvailExtendedVirtual", ctypes.c_ulonglong),
                ]

            stat = MEMORYSTATUSEX()
            stat.dwLength = ctypes.sizeof(MEMORYSTATUSEX)
            ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(stat))
            mem_gib = int(stat.ullTotalPhys / (1024**3))
        except Exception:
            mem_gib = 0
    else:
        tier = f"unknown_{system}_{machine}".lower()

    # Coarse labels only — never hostname/username/serial.
    return {
        "profile_id": profile_id,
        "tier": tier,
        "cpu_label": cpu_label,
        "core_count": cores,
        "memory_gib": mem_gib,
        "os_label": os_label,
        "power_mode": "default",
    }


def time_cmd(cmd: list[str], env: dict | None = None) -> tuple[float, int, str]:
    t0 = time.perf_counter()
    r = subprocess.run(cmd, capture_output=True, text=True, env=env)
    ms = (time.perf_counter() - t0) * 1000.0
    err = (r.stderr or "")[-400:]
    return ms, r.returncode, err


def audio_duration_ms(path: Path) -> float:
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
        ).strip()
        return float(out) * 1000.0
    except Exception:
        return 0.0


def make_approx_duration_wav(src: Path, dest: Path, target_secs: float) -> Path:
    """Loop-concat a short fixture to approximate target duration."""
    if dest.is_file():
        return dest
    dest.parent.mkdir(parents=True, exist_ok=True)
    src_dur = max(audio_duration_ms(src) / 1000.0, 0.1)
    n = max(1, int(target_secs / src_dur) + 1)
    # Use ffmpeg amovie loop
    cmd = [
        "ffmpeg",
        "-y",
        "-stream_loop",
        str(n - 1),
        "-i",
        str(src),
        "-t",
        str(target_secs),
        "-ac",
        "1",
        "-ar",
        "16000",
        "-c:a",
        "pcm_s16le",
        str(dest),
    ]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0 or not dest.is_file():
        raise SystemExit(f"ffmpeg loop failed: {r.stderr[-300:]}")
    return dest


def scenario_result(
    scenario_id: str,
    samples_ms: list[float],
    *,
    audio_ms: float | None,
    warm: bool,
    release_gated: bool,
    concurrency: int = 1,
) -> dict:
    samples = sorted(samples_ms)
    p50 = percentile(samples, 0.50)
    p95 = percentile(samples, 0.95)
    mean = statistics.fmean(samples) if samples else 0.0
    rtf = (p50 / audio_ms) if audio_ms and audio_ms > 0 else None
    return {
        "scenario_id": scenario_id,
        "samples_ms": [round(s, 3) for s in samples],
        "p50_ms": round(p50, 3),
        "p95_ms": round(p95, 3),
        "mean_ms": round(mean, 3),
        "audio_or_synth_duration_ms": round(audio_ms, 3) if audio_ms else None,
        "rtf_p50": round(rtf, 4) if rtf is not None else None,
        "concurrency": concurrency,
        "warm": warm,
        "release_gated": release_gated,
    }


def find_model_digest(cache_roots: list[Path], model: str) -> str | None:
    # ggml-tiny-q5_1.bin etc.
    name = f"ggml-{model}.bin"
    for root in cache_roots:
        p = root / "models" / name
        if p.is_file():
            return sha256_file(p)
        # sha256 sidecar
        side = root / "models" / f"{name}.sha256"
        if side.is_file():
            return side.read_text(encoding="utf-8").strip().split()[0]
    return None


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--profile-id",
        default=None,
        help="tier_a_macos_arm64 | tier_a_linux_x86_64_gnu | tier_a_windows_x86_64_msvc",
    )
    ap.add_argument("--aurum", default=os.environ.get("AURUM_BIN", "aurum"))
    ap.add_argument("--stt-model", default="tiny-q5_1")
    ap.add_argument("--fixture", default="tests/fixtures/sample.wav")
    ap.add_argument("--out", default=None)
    ap.add_argument("--out-dir", default="evals/reports/perf")
    ap.add_argument("--reps-workflow", type=int, default=20)
    ap.add_argument("--reps-stt", type=int, default=5)
    ap.add_argument("--reps-tts", type=int, default=5)
    ap.add_argument("--warmups", type=int, default=2)
    ap.add_argument("--skip-tts", action="store_true")
    ap.add_argument("--skip-30s", action="store_true")
    ap.add_argument("--local-only", action="store_true", default=True)
    args = ap.parse_args()

    root = Path.cwd()
    aurum = args.aurum
    if not Path(aurum).is_file() and not shutil.which(aurum):
        raise SystemExit(f"aurum not found: {aurum}")

    system = platform.system()
    if args.profile_id:
        profile_id = args.profile_id
    elif system == "Darwin":
        profile_id = "tier_a_macos_arm64"
    elif system == "Linux":
        profile_id = "tier_a_linux_x86_64_gnu"
    elif system == "Windows":
        profile_id = "tier_a_windows_x86_64_msvc"
    else:
        profile_id = f"tier_a_{system.lower()}"

    hardware = detect_hardware(profile_id)
    fixture = (root / args.fixture).resolve()
    if not fixture.is_file():
        raise SystemExit(f"missing fixture {fixture}")

    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    version = (root / "VERSION").read_text(encoding="utf-8").strip()

    cache_roots = []
    if system == "Darwin":
        cache_roots.append(Path.home() / "Library/Caches/aurum")
    cache_roots.append(Path.home() / ".cache/aurum")
    if os.environ.get("AURUM_CACHE_DIR"):
        cache_roots.insert(0, Path(os.environ["AURUM_CACHE_DIR"]))

    model_digests = {}
    dig = find_model_digest(cache_roots, args.stt_model)
    if dig:
        model_digests[args.stt_model] = dig

    scenarios: list[dict] = []
    tmp = Path(tempfile.mkdtemp(prefix="aurum-perf-"))
    try:
        # --- workflow/doctor_startup ---
        print(f"=== workflow/doctor_startup ({args.reps_workflow} samples) ===")
        samples = []
        for i in range(args.warmups):
            time_cmd([aurum, "doctor"])
        for i in range(args.reps_workflow):
            ms, code, err = time_cmd([aurum, "doctor"])
            if code != 0:
                print(f"  WARN doctor failed: {err}", file=sys.stderr)
            samples.append(ms)
            print(f"  run {i+1}: {ms:.1f} ms")
        scenarios.append(
            scenario_result(
                "workflow/doctor_startup",
                samples,
                audio_ms=None,
                warm=True,
                release_gated=True,
            )
        )

        # --- workflow/cli_stt_one_file ---
        print(f"=== workflow/cli_stt_one_file model={args.stt_model} ===")
        out_txt = tmp / "stt_out.txt"
        samples = []
        cmd_base = [
            aurum,
            str(fixture),
            "--model",
            args.stt_model,
            "--output-file",
            str(out_txt),
            "--language",
            "en",
        ]
        if args.local_only:
            # no dedicated flag on STT for local-only in all versions; model must be cached
            pass
        for i in range(args.warmups):
            time_cmd(cmd_base)
        for i in range(args.reps_workflow):
            ms, code, err = time_cmd(cmd_base)
            if code != 0:
                raise SystemExit(f"STT failed: {err}")
            samples.append(ms)
            print(f"  run {i+1}: {ms:.1f} ms")
        audio_ms = audio_duration_ms(fixture)
        scenarios.append(
            scenario_result(
                "workflow/cli_stt_one_file",
                samples,
                audio_ms=audio_ms,
                warm=True,
                release_gated=True,
            )
        )

        # --- stt_local/tiny-q5_1/30s/warm (approx 30s loop fixture) ---
        if not args.skip_30s and shutil.which("ffmpeg"):
            print(f"=== stt_local/{args.stt_model}/30s/warm ===")
            wav30 = tmp / "approx_30s.wav"
            make_approx_duration_wav(fixture, wav30, 30.0)
            a30 = audio_duration_ms(wav30)
            samples = []
            cmd30 = [
                aurum,
                str(wav30),
                "--model",
                args.stt_model,
                "--output-file",
                str(tmp / "stt30.txt"),
                "--language",
                "en",
            ]
            for i in range(1):  # one warmup for longer clip
                time_cmd(cmd30)
            for i in range(args.reps_stt):
                ms, code, err = time_cmd(cmd30)
                if code != 0:
                    print(f"  WARN 30s STT failed: {err}", file=sys.stderr)
                    continue
                samples.append(ms)
                print(f"  run {i+1}: {ms:.1f} ms rtf={ms/a30:.4f}")
            if samples:
                scenarios.append(
                    scenario_result(
                        f"stt_local/{args.stt_model}/30s/warm",
                        samples,
                        audio_ms=a30,
                        warm=True,
                        release_gated=args.stt_model in ("tiny-q5_1", "base"),
                    )
                )

        # --- TTS short ---
        if not args.skip_tts:
            print("=== tts_local/kitten-nano-int8/Luna/short ===")
            samples = []
            tts_out = tmp / "tts_short.wav"
            tts_cmd = [
                aurum,
                "tts",
                "Hello from Aurum.",
                "--model",
                "kitten-nano-int8",
                "--voice",
                "Luna",
                "--output-file",
                str(tts_out),
                "--force",
            ]
            if args.local_only:
                tts_cmd.append("--local-only")
            for i in range(args.warmups):
                time_cmd(tts_cmd)
            for i in range(args.reps_tts):
                ms, code, err = time_cmd(tts_cmd)
                if code != 0:
                    print(f"  WARN TTS failed (skip TTS scenarios): {err}", file=sys.stderr)
                    samples = []
                    break
                samples.append(ms)
                print(f"  run {i+1}: {ms:.1f} ms")
            if samples and tts_out.is_file():
                synth_ms = audio_duration_ms(tts_out)
                scenarios.append(
                    scenario_result(
                        "tts_local/kitten-nano-int8/Luna/short",
                        samples,
                        audio_ms=synth_ms,
                        warm=True,
                        release_gated=True,
                    )
                )

    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    report = {
        "schema_version": 2,
        "evidence_version": "0.0.22-perf-v1",
        "kind": "tier_a_perf_capture",
        "hardware": hardware,
        "aurum_version": version,
        "commit": commit,
        "target_triple": platform.machine(),
        "build_profile": "operator_binary",
        "model_digests": model_digests,
        "cache_state": "prewarmed_local" if model_digests else "unknown",
        "scenarios": scenarios,
        "notes": (
            "JOE-2317 field capture. Hostnames/user paths redacted. "
            "30s STT uses looped short fixture (not unique speech content). "
            "Cross-machine comparisons are informational only."
        ),
    }

    out_dir = root / args.out_dir
    out_dir.mkdir(parents=True, exist_ok=True)
    if args.out:
        out_path = Path(args.out)
        if not out_path.is_absolute():
            out_path = root / out_path
    else:
        out_path = out_dir / f"perf-{profile_id}-field.json"
    out_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    # Also write a budget seed from measured p50/p95 for this machine (operator review).
    budget = {
        "schema_version": 2,
        "evidence_version": "0.0.22-perf-v1",
        "hardware_profile_id": profile_id,
        "notes": (
            f"Seeded from field capture {out_path.name} commit={commit[:12]}. "
            "Review before treating as release gate baseline."
        ),
        "source_report": str(out_path.relative_to(root)) if str(out_path.resolve()).startswith(str(root.resolve())) else str(out_path),
        "scenarios": [],
    }
    for s in scenarios:
        if not s.get("release_gated"):
            continue
        budget["scenarios"].append(
            {
                "scenario_id": s["scenario_id"],
                "baseline_p50_ms": s["p50_ms"],
                "baseline_p95_ms": s["p95_ms"],
                "baseline_rtf_p50": s.get("rtf_p50"),
                "max_p50_relative_warn": 0.10,
                "max_p95_relative_fail": 0.15,
                "max_rss_relative_fail": 0.15,
                "max_rss_absolute_bytes": 268435456,
                "max_throughput_relative_drop": 0.15,
            }
        )
    budget_path = out_dir / f"budget-seed-{profile_id}.json"
    budget_path.write_text(json.dumps(budget, indent=2) + "\n", encoding="utf-8")

    print(f"WROTE {out_path}")
    print(f"WROTE {budget_path} ({len(budget['scenarios'])} release-gated scenarios)")
    for s in scenarios:
        print(
            f"  {s['scenario_id']}: p50={s['p50_ms']:.1f} p95={s['p95_ms']:.1f} "
            f"rtf={s.get('rtf_p50')}"
        )


if __name__ == "__main__":
    main()
