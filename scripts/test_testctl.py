#!/usr/bin/env python3
"""Regression tests for test-runner outcomes and discovery integrity."""
from __future__ import annotations

import contextlib
import io
import json
import subprocess
import sys
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path
from unittest.mock import patch

import testctl


def args(**overrides):
    values = dict(
        segment=None, graphical=False, full=False, nightly=False,
        allow_display=False, offline=False, budget=None,
    )
    values.update(overrides)
    return Namespace(**values)


def segment(sid, **overrides):
    value = dict(
        id=sid, kind="test", default=True, expected_s=1,
        run=[sys.executable, "-c", "pass"],
        list=[sys.executable, "-c", f"print({sid!r})"],
    )
    value.update(overrides)
    return value


class TestRunner(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="aurum-testctl-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.stdout = io.StringIO()
        self.stderr = io.StringIO()
        self.enterContext(contextlib.redirect_stdout(self.stdout))
        self.enterContext(contextlib.redirect_stderr(self.stderr))

    def test_budget_preserves_defaults_and_explicit_segments(self):
        default = segment("default", expected_s=2)
        explicit = segment("explicit", default=False, expected_s=2)
        optional = segment("optional", default=False, expected_s=5)
        keep, dropped, fits = testctl.json_budget(
            [default, explicit, optional], args(segment=["explicit"], budget=3)
        )
        self.assertEqual(keep, [default, explicit])
        self.assertEqual(dropped, [optional])
        self.assertFalse(fits)

    def test_mixed_success_and_skip_is_partial(self):
        manifest = {"segments": [segment("ok"), segment("online", requires=["network"])]}
        result = testctl.cmd_run(manifest, args(offline=True), self.root, "run")
        self.assertEqual(result, testctl.EXIT_SKIP)
        self.assertIn("PARTIAL", self.stderr.getvalue())

    def test_measure_preserves_failure_exit_and_evidence(self):
        manifest = {"segments": [segment("bad", run=[sys.executable, "-c", "raise SystemExit(7)"])]}
        result = testctl.cmd_run(manifest, args(), self.root, "measure")
        self.assertEqual(result, testctl.EXIT_FAIL)
        record = json.loads((self.root / "test-segments.lock.json").read_text())
        self.assertEqual(record["segments"]["bad"]["exit"], 7)
        self.assertEqual(record["segments"]["bad"]["test_count"], 1)

    def test_measure_preserves_partial_exit(self):
        manifest = {"segments": [segment("ok"), segment("online", requires=["network"])]}
        result = testctl.cmd_run(manifest, args(offline=True), self.root, "measure")
        self.assertEqual(result, testctl.EXIT_SKIP)
        self.assertTrue((self.root / "test-segments.lock.json").is_file())
        record = json.loads((self.root / "test-segments.lock.json").read_text())
        self.assertEqual(record["skipped"], [{"id": "online", "reason": "network (--offline)"}])

    def test_measure_reports_test_count_discovery_failure(self):
        manifest = {"segments": [segment("bad-list", list=[sys.executable, "-c", "raise SystemExit(9)"])]}
        result = testctl.cmd_run(manifest, args(), self.root, "measure")
        self.assertEqual(result, testctl.EXIT_FAIL)
        record = json.loads((self.root / "test-segments.lock.json").read_text())
        self.assertIn("test discovery exited 9", record["segments"]["bad-list"]["discovery_error"])

    def test_verify_rejects_failed_universe_with_partial_output(self):
        manifest = {
            "meta": {"universe": [sys.executable, "-c", "print('one'); raise SystemExit(7)"]},
            "segments": [segment("one")],
        }
        self.assertEqual(testctl.cmd_verify(manifest, args(), self.root), testctl.EXIT_FAIL)
        self.assertIn("universe: test discovery exited 7", self.stderr.getvalue())

    def test_verify_rejects_failed_listing_even_if_other_segment_covers_universe(self):
        manifest = {
            "meta": {"universe": [sys.executable, "-c", "print('one')"]},
            "segments": [segment("one"), segment("broken", list=[sys.executable, "-c", "raise SystemExit(9)"])],
        }
        self.assertEqual(testctl.cmd_verify(manifest, args(), self.root), testctl.EXIT_FAIL)
        self.assertIn("broken: test discovery exited 9", self.stderr.getvalue())

    def test_verify_reports_missing_discovery_command(self):
        manifest = {"meta": {"universe": [str(self.root / "missing")]}, "segments": []}
        self.assertEqual(testctl.cmd_verify(manifest, args(), self.root), testctl.EXIT_FAIL)

    def test_verify_bounds_discovery_time(self):
        manifest = {"meta": {"universe": ["blocked-list"]}, "segments": []}
        with patch.object(testctl.subprocess, "run", side_effect=subprocess.TimeoutExpired("blocked-list", 300)) as run:
            self.assertEqual(testctl.cmd_verify(manifest, args(), self.root), testctl.EXIT_FAIL)
        self.assertEqual(run.call_args.kwargs["timeout"], 300)

    def test_verify_partitions_nextest_ignored_tests(self):
        payload = json.dumps({"suite": {"binary-id": "core", "testcases": {
            "ordinary": {"ignored": False}, "real-model": {"ignored": True},
        }}})
        listing = [sys.executable, "-c", f"print({payload!r})"]
        manifest = {
            "meta": {"universe": listing},
            "segments": [segment("ordinary", list=listing), segment("models", list=listing, ignored="only")],
        }
        self.assertEqual(testctl.cmd_verify(manifest, args(), self.root), testctl.EXIT_OK)
        self.assertIn("universe: 2 tests (1 ignored)", self.stdout.getvalue())


def test_ids(suite):
    for test in suite:
        if isinstance(test, unittest.TestSuite):
            yield from test_ids(test)
        else:
            yield "testctl::" + test.id().split(".", 1)[1]


if __name__ == "__main__":
    if sys.argv[1:] == ["--list"]:
        print("\n".join(test_ids(unittest.defaultTestLoader.loadTestsFromTestCase(TestRunner))))
    else:
        unittest.main()
