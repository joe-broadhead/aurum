#!/usr/bin/env python3
"""Prepare a blinded TTS listening session pack (JOE-2319).

Generates offline WAVs for selected fixtures × model/voice pairs, assigns
opaque blinded labels, writes a rating capture template, and can aggregate
filled ratings into a ListeningReport (no listener PII).

Does **not** invent human scores. Three independent listeners must fill
ratings JSONL before aggregate is evidence for support-tier promotion.

Commands:
  prepare   — synth audio + write session manifest + empty ratings template
  aggregate — map blinded labels → models and write aggregate report

Example:
  python3 scripts/eval/prepare_tts_listening_session.py prepare \\
    --pairs kitten-nano-int8:Luna,kitten-nano-int8:Jasper \\
    --min-fixtures 20
"""
from __future__ import annotations

import argparse
import csv
import json
import os
import random
import secrets
import subprocess
import sys
from collections import defaultdict
from pathlib import Path


def median_u8(vals: list[int]) -> float:
    if not vals:
        return 0.0
    s = sorted(vals)
    n = len(s)
    if n % 2:
        return float(s[n // 2])
    return (s[n // 2 - 1] + s[n // 2]) / 2.0


def select_fixtures(pack: dict, min_n: int) -> list[dict]:
    """Pick ≥min_n representative fixtures across categories."""
    fixtures = [
        f
        for f in pack.get("fixtures") or []
        if (f.get("participation") or "both") in ("both", "listening_only")
        and "invalid_input" not in [t.lower() for t in f.get("tags") or []]
        and "control" not in [t.lower() for t in f.get("tags") or []]
    ]
    # Prefer diversity by primary tag
    by_tag: dict[str, list[dict]] = defaultdict(list)
    for f in fixtures:
        tags = f.get("tags") or ["misc"]
        by_tag[tags[0]].append(f)
    selected: list[dict] = []
    # Round-robin categories
    while len(selected) < min_n and any(by_tag.values()):
        for tag in sorted(by_tag.keys()):
            if by_tag[tag] and len(selected) < min_n:
                selected.append(by_tag[tag].pop(0))
        if all(not v for v in by_tag.values()):
            break
    # pad if needed
    remaining = [f for f in fixtures if f not in selected]
    for f in remaining:
        if len(selected) >= min_n:
            break
        selected.append(f)
    return selected


def synth(aurum: str, text: str, model: str, voice: str, out: Path, local_only: bool) -> bool:
    out.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        aurum,
        "tts",
        text,
        "--model",
        model,
        "--voice",
        voice,
        "--output-file",
        str(out),
        "--force",
    ]
    if local_only:
        cmd.append("--local-only")
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        print(f"  WARN synth failed {model}/{voice}: {r.stderr[-200:]}", file=sys.stderr)
        return False
    return out.is_file()


def cmd_prepare(args: argparse.Namespace) -> None:
    root = Path.cwd()
    pack = json.loads((root / args.pack).read_text(encoding="utf-8"))
    fixtures = select_fixtures(pack, args.min_fixtures)
    if len(fixtures) < args.min_fixtures:
        print(
            f"WARN only {len(fixtures)} listening fixtures available (wanted {args.min_fixtures})",
            file=sys.stderr,
        )

    pairs = []
    for p in args.pairs.split(","):
        p = p.strip()
        if not p:
            continue
        m, v = p.split(":", 1)
        pairs.append((m.strip(), v.strip()))
    if len(pairs) < 1:
        raise SystemExit("need at least one model:voice pair")

    session_id = args.session_id or f"listening-{secrets.token_hex(3)}"
    session_dir = root / args.out_dir / session_id
    audio_dir = session_dir / "audio"
    session_dir.mkdir(parents=True, exist_ok=True)

    # Blinded labels: one letter per pair, shuffled
    labels = [chr(ord("A") + i) for i in range(len(pairs))]
    rng = random.Random(args.seed)
    rng.shuffle(labels)
    label_to_pair = {lab: pairs[i] for i, lab in enumerate(labels)}
    # Reveal map is operator-secret until after ratings collected
    reveal = {
        lab: {"model": m, "voice": v} for lab, (m, v) in label_to_pair.items()
    }

    items = []  # listening items in presentation order
    for fix in fixtures:
        for lab, (model, voice) in label_to_pair.items():
            wav_name = f"{fix['id']}__{lab}.wav"
            wav_path = audio_dir / wav_name
            print(f"  synth {fix['id']} label={lab} ({model}/{voice})")
            ok = synth(args.aurum, fix["text"], model, voice, wav_path, args.local_only)
            if not ok:
                continue
            items.append(
                {
                    "item_id": f"{fix['id']}__{lab}",
                    "fixture_id": fix["id"],
                    "blinded_label": lab,
                    "audio_relpath": f"audio/{wav_name}",
                    "language": fix.get("language"),
                    "tags": fix.get("tags") or [],
                    # text intentionally omitted from public aggregate; kept in session
                    # for operator playback guidance only when --include-text
                    **(
                        {"text": fix["text"]}
                        if args.include_text
                        else {}
                    ),
                }
            )

    rng.shuffle(items)

    session = {
        "schema_version": 1,
        "evidence_version": "0.0.22-tts-listening-v1",
        "kind": "tts_listening_session",
        "session_id": session_id,
        "protocol_version": "tts-listening-v1",
        "blinding": True,
        "listener_count_required": 3,
        "scales": {
            "intelligibility": "1-5",
            "naturalness": "1-5",
            "pronunciation": "1-5",
            "join_smoothness": "1-5",
            "critical_failure": "bool — omitted words, severe mispronunciation, clipping, unusable",
        },
        "playback_guidance": (
            "Identical volume normalization; headphones preferred; "
            "same session guidance for all listeners. Rate each item independently."
        ),
        "privacy": (
            "Listener id must be opaque (e.g. L1/L2/L3). "
            "No name, email, or device serial in ratings files."
        ),
        "fixture_count": len(fixtures),
        "pair_count": len(pairs),
        "item_count": len(items),
        "items": items,
        "notes": (
            "Reveal map is in reveal.json — do not share with listeners until "
            "all ratings are collected. Session audio is operator-local (not for public CI)."
        ),
    }
    (session_dir / "session.json").write_text(
        json.dumps(session, indent=2) + "\n", encoding="utf-8"
    )
    (session_dir / "reveal.json").write_text(
        json.dumps(
            {
                "session_id": session_id,
                "label_to_model": reveal,
                "warning": "OPERATOR ONLY — reveal after ratings complete",
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )

    # Ratings template JSONL (one line per listener×item after fill)
    template_header = {
        "session_id": session_id,
        "listener_id": "L?",
        "item_id": "",
        "fixture_id": "",
        "blinded_label": "",
        "intelligibility": None,
        "naturalness": None,
        "pronunciation": None,
        "join_smoothness": None,
        "critical_failure": False,
        "notes": "",
    }
    (session_dir / "ratings.template.jsonl").write_text(
        json.dumps(template_header) + "\n", encoding="utf-8"
    )
    # CSV worksheet for human-friendly capture
    csv_path = session_dir / "ratings_worksheet.csv"
    with csv_path.open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(
            [
                "listener_id",
                "item_id",
                "fixture_id",
                "blinded_label",
                "audio_relpath",
                "intelligibility_1to5",
                "naturalness_1to5",
                "pronunciation_1to5",
                "join_smoothness_1to5",
                "critical_failure_yes_no",
                "notes",
            ]
        )
        for it in items:
            w.writerow(
                [
                    "",
                    it["item_id"],
                    it["fixture_id"],
                    it["blinded_label"],
                    it["audio_relpath"],
                    "",
                    "",
                    "",
                    "",
                    "no",
                    "",
                ]
            )

    readme = session_dir / "README.md"
    readme.write_text(
        f"""# TTS listening session `{session_id}`

Evidence version: `0.0.22-tts-listening-v1` (JOE-2319)

## Required listeners: **3**

1. Give each listener the `audio/` folder + `ratings_worksheet.csv` (or JSONL).
2. Do **not** share `reveal.json` until all three rating files are collected.
3. Listeners use opaque ids only (`L1`, `L2`, `L3`).
4. After collection, copy filled worksheets to `ratings_L1.csv` … or a single
   `ratings.jsonl` with one object per rating.
5. Aggregate:

```bash
python3 scripts/eval/prepare_tts_listening_session.py aggregate \\
  --session-dir {session_dir.relative_to(root)} \\
  --ratings {session_dir.relative_to(root)}/ratings.jsonl
```

## Items

- Fixtures: {len(fixtures)}
- Blinded systems: {len(pairs)} → labels {sorted(reveal)}
- Presentation items: {len(items)} (shuffled)

## Honesty

Empty ratings are not product evidence. Single-listener pilot rounds are not a
three-listener blinded study.
""",
        encoding="utf-8",
    )
    print(f"WROTE session {session_dir}")
    print(f"  items={len(items)} labels={reveal}")
    print("  Next: recruit 3 listeners; then aggregate.")


def load_ratings(path: Path) -> list[dict]:
    if path.suffix == ".jsonl":
        rows = []
        for line in path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line:
                continue
            rows.append(json.loads(line))
        return rows
    if path.suffix == ".csv":
        rows = []
        with path.open(newline="", encoding="utf-8") as f:
            for r in csv.DictReader(f):
                if not (r.get("intelligibility_1to5") or r.get("intelligibility")):
                    continue
                rows.append(
                    {
                        "listener_id": r.get("listener_id") or "L?",
                        "item_id": r.get("item_id"),
                        "fixture_id": r.get("fixture_id"),
                        "blinded_label": r.get("blinded_label"),
                        "intelligibility": int(
                            r.get("intelligibility_1to5") or r.get("intelligibility")
                        ),
                        "naturalness": int(
                            r.get("naturalness_1to5") or r.get("naturalness")
                        ),
                        "pronunciation": int(
                            r.get("pronunciation_1to5") or r.get("pronunciation")
                        ),
                        "join_smoothness": int(
                            r.get("join_smoothness_1to5") or r.get("join_smoothness")
                        ),
                        "critical_failure": str(
                            r.get("critical_failure_yes_no")
                            or r.get("critical_failure")
                            or ""
                        )
                        .lower()
                        in ("yes", "true", "1", "y"),
                        "notes": r.get("notes") or "",
                    }
                )
        return rows
    raise SystemExit(f"unsupported ratings format: {path}")


def cmd_aggregate(args: argparse.Namespace) -> None:
    root = Path.cwd()
    session_dir = root / args.session_dir
    session = json.loads((session_dir / "session.json").read_text(encoding="utf-8"))
    reveal = json.loads((session_dir / "reveal.json").read_text(encoding="utf-8"))
    label_map = reveal["label_to_model"]

    rating_paths = [Path(p) for p in args.ratings]
    all_rows: list[dict] = []
    for p in rating_paths:
        if not p.is_absolute():
            p = root / p
        all_rows.extend(load_ratings(p))

    listeners = {r.get("listener_id") for r in all_rows if r.get("listener_id")}
    by_model: dict[str, list[dict]] = defaultdict(list)
    for r in all_rows:
        lab = r.get("blinded_label")
        if lab not in label_map:
            print(f"WARN unknown label {lab}", file=sys.stderr)
            continue
        model = label_map[lab]["model"]
        voice = label_map[lab]["voice"]
        key = f"{model}/{voice}"
        by_model[key].append(r)

    aggregates = {}
    critical_total = 0
    for model, ratings in sorted(by_model.items()):
        intel = [int(r["intelligibility"]) for r in ratings]
        nat = [int(r["naturalness"]) for r in ratings]
        pro = [int(r["pronunciation"]) for r in ratings]
        join = [int(r["join_smoothness"]) for r in ratings]
        crit = sum(1 for r in ratings if r.get("critical_failure"))
        critical_total += crit
        aggregates[model] = {
            "n_ratings": len(ratings),
            "median_intelligibility": median_u8(intel),
            "median_naturalness": median_u8(nat),
            "median_pronunciation": median_u8(pro),
            "median_join_smoothness": median_u8(join),
            "critical_failures": crit,
        }

    listener_count = len(listeners)
    report = {
        "schema_version": 1,
        "evidence_version": "0.0.22-tts-listening-v1",
        "kind": "tts_listening_report",
        "round_id": session.get("session_id"),
        "protocol_version": session.get("protocol_version"),
        "listener_count": listener_count,
        "blinding": True,
        "playback_normalization": "operator session guidance (headphones preferred)",
        "model_aggregates": aggregates,
        "critical_failure_count": critical_total,
        "meets_min_listeners": listener_count >= 3,
        "notes": (
            None
            if listener_count >= 3
            else f"Only {listener_count} listener id(s); need ≥3 for support-tier promotion."
        ),
    }
    out = (
        root
        / args.report_dir
        / f"listening-aggregate-{session.get('session_id')}.json"
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"WROTE {out}")
    print(json.dumps(aggregates, indent=2))
    if listener_count < 3:
        print(
            f"HONESTY: listener_count={listener_count} < 3 — not promotion-ready",
            file=sys.stderr,
        )
        sys.exit(2)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("prepare", help="Generate blinded session audio + templates")
    p.add_argument("--pack", default="evals/observatory/tts_eval_pack.v1.json")
    p.add_argument(
        "--pairs",
        default="kitten-nano-int8:Luna,kitten-nano-int8:Jasper",
        help="model:voice pairs to blind against each other",
    )
    p.add_argument("--min-fixtures", type=int, default=20)
    p.add_argument("--out-dir", default="evals/reports/_local/tts_listening_sessions")
    p.add_argument("--session-id", default=None)
    p.add_argument("--seed", type=int, default=42)
    p.add_argument("--aurum", default=os.environ.get("AURUM_BIN", "aurum"))
    p.add_argument("--local-only", action="store_true")
    p.add_argument(
        "--include-text",
        action="store_true",
        help="Embed source text in session.json (operator convenience; omit for stricter blinding)",
    )

    a = sub.add_parser("aggregate", help="Aggregate filled ratings after reveal")
    a.add_argument("--session-dir", required=True)
    a.add_argument(
        "--ratings",
        nargs="+",
        required=True,
        help="One or more ratings.jsonl / ratings_L*.csv files",
    )
    a.add_argument("--report-dir", default="evals/reports/listening")

    args = ap.parse_args()
    if args.cmd == "prepare":
        cmd_prepare(args)
    else:
        cmd_aggregate(args)


if __name__ == "__main__":
    main()
