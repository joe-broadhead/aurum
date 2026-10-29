#!/usr/bin/env python3
"""List the eval-helper tests for the repository test universe."""

from __future__ import annotations

import unittest
from pathlib import Path


def walk(suite: unittest.TestSuite):
    for test in suite:
        if isinstance(test, unittest.TestSuite):
            yield from walk(test)
        else:
            yield test.id()


def main() -> None:
    start_dir = Path(__file__).resolve().parent
    suite = unittest.defaultTestLoader.discover(str(start_dir), pattern="test_*.py")
    print("\n".join(sorted(walk(suite))))


if __name__ == "__main__":
    main()
