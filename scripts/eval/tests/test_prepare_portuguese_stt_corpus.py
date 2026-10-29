#!/usr/bin/env python3
"""Offline tests for the CORAA pt-BR selection invariant."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from prepare_portuguese_stt_corpus import group_pt_br_rows


class GroupPtBrRowsTests(unittest.TestCase):
    def test_excludes_pt_pt_rows_before_grouping(self) -> None:
        rows = [
            # Sorts first by sha256(file_path); it must still be excluded.
            {
                "file_path": "test/Ted_part1/aIndPtZGDz4-0127.wav",
                "variety": "pt_pt",
                "dataset": "TEDx Talks",
                "accent": "Misc.",
            },
            {
                "file_path": "test/sp/42881_sp_.wav",
                "variety": "pt_br",
                "dataset": "SP2010",
                "accent": "São Paulo (cap.)",
            },
        ]

        groups = group_pt_br_rows(rows)
        selected = [row["file_path"] for group in groups.values() for row in group]

        self.assertEqual(selected, ["test/sp/42881_sp_.wav"])
        self.assertFalse(
            any(row["variety"] != "pt_br" for group in groups.values() for row in group)
        )

    def test_excludes_rows_without_a_variety(self) -> None:
        rows = [
            {"file_path": "a.wav", "dataset": "d", "accent": "a"},
            {"file_path": "b.wav", "variety": "", "dataset": "d", "accent": "a"},
            {"file_path": "c.wav", "variety": "pt_br", "dataset": "d", "accent": "a"},
        ]

        groups = group_pt_br_rows(rows)
        selected = [row["file_path"] for group in groups.values() for row in group]

        self.assertEqual(selected, ["c.wav"])

    def test_accepts_normalized_variety_spelling(self) -> None:
        rows = [
            {"file_path": "a.wav", "variety": " PT_BR ", "dataset": "d", "accent": "a"}
        ]

        groups = group_pt_br_rows(rows)
        selected = [row["file_path"] for group in groups.values() for row in group]

        self.assertEqual(selected, ["a.wav"])


if __name__ == "__main__":
    unittest.main()
