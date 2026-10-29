#!/usr/bin/env python3
"""Offline tests for the Portuguese evaluation evidence guards."""

from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import compare_whisper_quantization as compare
import run_portuguese_stt_eval as runner


def write_mock(path: Path, body: str) -> Path:
    path.write_text("#!/usr/bin/env python3\n" + body + "\n", encoding="utf-8")
    path.chmod(0o755)
    return path


def write_manifest(path: Path, items: list[dict], clips_per_dialect: int = 1) -> None:
    manifest = {
        "schema_version": 1,
        "language": "pt",
        "selection": {"clips_per_dialect": clips_per_dialect},
        "items": items,
    }
    path.write_text(json.dumps(manifest), encoding="utf-8")


def compare_items() -> list[dict]:
    return [
        {
            "id": "br1",
            "dialect": "pt-BR",
            "audio": "ptbr.wav",
            "reference": "ola mundo",
            "duration_s": 5.0,
        },
        {
            "id": "pt1",
            "dialect": "pt-PT",
            "audio": "ptpt.wav",
            "reference": "ola mundo",
            "duration_s": 5.0,
        },
    ]


def runner_items() -> list[dict]:
    return [
        {
            "id": "br1",
            "dialect": "pt-BR",
            "audio": "ptbr.wav",
            "reference": "ola mundo",
            "duration_s": 5.0,
            "source_dataset": "gabrielrstan/CORAA-v1.1",
            "source_revision": "719c91226a79f5f9a8984145f15f29626eabc29a",
            "source_fixture_id": "test/sp/42881_sp_.wav",
        },
        {
            "id": "pt1",
            "dialect": "pt-PT",
            "audio": "ptpt.wav",
            "reference": "ola mundo",
            "duration_s": 5.0,
            "source_dataset": "inesc-id/camoes_SI",
            "source_revision": "c85aec9653738c2a58ead316b61e1438cf845bc5",
            "source_fixture_id": "1",
        },
    ]


class TranscriptGuardTests(unittest.TestCase):
    def test_zero_exit_without_transcript_is_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            transcript = Path(tmp) / "transcript.txt"
            status, hypothesis, error = runner.classify_transcript(
                0, "error: failed to read audio file\n", transcript
            )

        self.assertEqual(status, "error")
        self.assertEqual(hypothesis, "")
        self.assertIn("wrote no transcript", error)
        self.assertIn("failed to read audio file", error)

    def test_nonzero_exit_preserves_stderr(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            status, hypothesis, error = runner.classify_transcript(
                2, "boom\n", Path(tmp) / "transcript.txt"
            )

        self.assertEqual((status, hypothesis), ("error", ""))
        self.assertIn("boom", error)

    def test_existing_transcript_is_ok(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            transcript = Path(tmp) / "transcript.txt"
            transcript.write_text("ola mundo\n", encoding="utf-8")
            status, hypothesis, error = runner.classify_transcript(0, "", transcript)

        self.assertEqual((status, hypothesis, error), ("ok", "ola mundo", None))

    def test_fresh_run_dirs_are_never_reused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            parent = Path(tmp)
            first = runner.fresh_run_dir(parent, "A/pt-BR")
            second = runner.fresh_run_dir(parent, "A/pt-BR")

            self.assertNotEqual(first, second)
            self.assertTrue(first.is_dir())
            self.assertTrue(second.is_dir())

    def test_comparison_requires_two_successful_runs(self) -> None:
        rows = [
            {
                "label": "A",
                "status": "error",
                "normalized_hypothesis": "",
                "wer": 1.0,
                "cer": 1.0,
            },
            {
                "label": "B",
                "status": "error",
                "normalized_hypothesis": "",
                "wer": 1.0,
                "cer": 1.0,
            },
        ]

        self.assertIsNone(compare.build_comparison("pt-BR", rows))


@unittest.skipIf(os.name == "nt", "mock executables use a POSIX shebang")
class CompareScriptTests(unittest.TestCase):
    def run_compare(self, tmp: Path, cli_body: str, seed_stale: bool) -> dict:
        manifest = tmp / "manifest.json"
        write_manifest(manifest, compare_items())
        for name in ("ptbr.wav", "ptpt.wav"):
            (tmp / name).write_bytes(b"not audio")
        model = tmp / "model.bin"
        model.write_bytes(b"model")
        cli = write_mock(tmp / "whisper-cli", cli_body)
        out_dir = tmp / "out"
        out_dir.mkdir()
        if seed_stale:
            (out_dir / "A.txt").write_text("stale\n", encoding="utf-8")
            (out_dir / "B.txt").write_text("stale\n", encoding="utf-8")

        argv = [
            "compare",
            "--manifest",
            str(manifest),
            "--whisper-cli",
            str(cli),
            "--run",
            f"A,pt-BR,{model}",
            "--run",
            f"B,pt-BR,{model}",
            "--out-dir",
            str(out_dir),
        ]
        with patch.object(sys, "argv", argv):
            compare.main()
        return json.loads(
            (out_dir / "quantization-spotcheck.json").read_text(encoding="utf-8")
        )

    def test_zero_exit_without_output_skips_comparison(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report = self.run_compare(
                Path(tmp),
                'import sys\nsys.stderr.write("error: failed to read audio file\\n")\nsys.exit(0)',
                seed_stale=True,
            )

        self.assertEqual([row["status"] for row in report["results"]], ["error", "error"])
        self.assertEqual([row["hypothesis"] for row in report["results"]], ["", ""])
        self.assertEqual(report["comparisons"], [])
        self.assertIn("wrote no transcript", report["results"][0]["error"])
        self.assertIn("failed to read audio file", report["results"][0]["error"])

    def test_successful_runs_compare_their_own_output(self) -> None:
        body = (
            "import sys\n"
            "from pathlib import Path\n"
            "args = sys.argv[1:]\n"
            'prefix = args[args.index("-of") + 1]\n'
            'Path(prefix + ".txt").write_text("ola mundo\\n", encoding="utf-8")\n'
        )
        with tempfile.TemporaryDirectory() as tmp:
            report = self.run_compare(Path(tmp), body, seed_stale=False)

        self.assertEqual([row["status"] for row in report["results"]], ["ok", "ok"])
        self.assertEqual(
            [row["hypothesis"] for row in report["results"]],
            ["ola mundo", "ola mundo"],
        )
        self.assertEqual(len(report["comparisons"]), 1)
        self.assertTrue(report["comparisons"][0]["normalized_transcripts_equal"])
        self.assertEqual(report["comparisons"][0]["wer_delta_second_minus_first"], 0.0)

    def test_nonzero_exit_is_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report = self.run_compare(
                Path(tmp),
                'import sys\nsys.stderr.write("boom\\n")\nsys.exit(2)',
                seed_stale=False,
            )

        self.assertEqual([row["status"] for row in report["results"]], ["error", "error"])
        self.assertEqual(report["comparisons"], [])
        self.assertIn("boom", report["results"][0]["error"])


@unittest.skipIf(os.name == "nt", "mock executables use a POSIX shebang")
class RunnerScriptTests(unittest.TestCase):
    def run_runner(self, tmp: Path, aurum_body: str, seed_stale: bool) -> dict:
        manifest = tmp / "manifest.json"
        write_manifest(manifest, runner_items())
        for name in ("ptbr.wav", "ptpt.wav"):
            (tmp / name).write_bytes(b"not audio")
        aurum = write_mock(tmp / "aurum", aurum_body)
        out_dir = tmp / "out"
        transcripts = out_dir / "transcripts"
        transcripts.mkdir(parents=True)
        if seed_stale:
            (transcripts / "tiny-br1.txt").write_text("stale\n", encoding="utf-8")
            (transcripts / "tiny-pt1.txt").write_text("stale\n", encoding="utf-8")

        argv = [
            "runner",
            "--manifest",
            str(manifest),
            "--aurum",
            str(aurum),
            "--models",
            "tiny",
            "--out-dir",
            str(out_dir),
        ]
        with patch.object(sys, "argv", argv):
            runner.main()
        return json.loads(
            (out_dir / "portuguese-stt-results.json").read_text(encoding="utf-8")
        )

    def test_zero_exit_without_output_ignores_stale_transcripts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report = self.run_runner(
                Path(tmp),
                'import sys\nsys.stderr.write("error: failed to read audio file\\n")\nsys.exit(0)',
                seed_stale=True,
            )

        self.assertEqual([row["status"] for row in report["results"]], ["error", "error"])
        self.assertEqual([row["hypothesis"] for row in report["results"]], ["", ""])
        self.assertEqual(report["summary"]["tiny"]["pt-BR"]["errors"], 1)
        self.assertEqual(report["summary"]["tiny"]["pt-BR"]["completed"], 0)

    def test_successful_run_reads_its_own_transcript(self) -> None:
        body = (
            "import sys\n"
            "from pathlib import Path\n"
            "args = sys.argv[1:]\n"
            'out = args[args.index("--output-file") + 1]\n'
            'Path(out).write_text("ola mundo\\n", encoding="utf-8")\n'
        )
        with tempfile.TemporaryDirectory() as tmp:
            report = self.run_runner(Path(tmp), body, seed_stale=False)

        self.assertEqual([row["status"] for row in report["results"]], ["ok", "ok"])
        self.assertEqual(
            [row["hypothesis"] for row in report["results"]],
            ["ola mundo", "ola mundo"],
        )
        self.assertEqual(report["summary"]["tiny"]["pt-BR"]["completed"], 1)


if __name__ == "__main__":
    unittest.main()
