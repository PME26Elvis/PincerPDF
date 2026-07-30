"""Regression tests for the P4.1 Merge evidence summarizer."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "summarize-merge-evidence.py"
SPEC = importlib.util.spec_from_file_location("summarize_merge_evidence", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class FirstNonEmptyLineTests(unittest.TestCase):
    """Cover version tools that choose different output streams."""

    def test_prefers_stdout_when_present(self) -> None:
        self.assertEqual(
            MODULE.first_non_empty_line("\nqpdf version 11.3.0\n", "ignored"),
            "qpdf version 11.3.0",
        )

    def test_falls_back_to_stderr(self) -> None:
        self.assertEqual(
            MODULE.first_non_empty_line("", "\nmutool version 1.21.1\n"),
            "mutool version 1.21.1",
        )

    def test_rejects_missing_version_output(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "no version output"):
            MODULE.first_non_empty_line("", " \n ")


if __name__ == "__main__":
    unittest.main()
