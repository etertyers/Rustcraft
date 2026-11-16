#!/usr/bin/env python3
"""Tests for scripts/perf_gate.py's baseline choice: python3 scripts/test_perf_gate.py"""
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import perf_gate  # noqa: E402

ROOT = HERE.parent


def write(dir_, name, machine, rows=None):
    (Path(dir_) / name).write_text(json.dumps({"machine": machine, "rows": rows or {}}))


class PickBaseline(unittest.TestCase):
    def test_the_file_for_the_results_machine_is_chosen(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "baseline.json", "Macmini9,1")
            write(d, "baseline-m4.json", "Mac16,12")
            self.assertEqual(perf_gate.pick_baseline("Macmini9,1", d).name, "baseline.json")
            self.assertEqual(perf_gate.pick_baseline("Mac16,12", d).name, "baseline-m4.json")

    def test_an_unknown_machine_gets_baseline_json_which_refuses_it(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "baseline.json", "Macmini9,1")
            write(d, "baseline-m4.json", "Mac16,12")
            self.assertEqual(perf_gate.pick_baseline("Mac99,9", d).name, "baseline.json")

    def test_unreadable_files_are_skipped(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "baseline.json", "Macmini9,1")
            (Path(d) / "baseline-broken.json").write_text("not json")
            write(d, "baseline-m4.json", "Mac16,12")
            self.assertEqual(perf_gate.pick_baseline("Mac16,12", d).name, "baseline-m4.json")


class RealFiles(unittest.TestCase):
    def run_gate(self, results):
        return subprocess.run([sys.executable, str(HERE / "perf_gate.py"), str(results)], capture_output=True, text=True, cwd=ROOT)

    def test_the_m4_file_is_gated_against_the_m4_baseline(self):
        r = self.run_gate(ROOT / "docs/perf/v1.6-check.json")
        self.assertIn("baseline-m4.json", r.stdout)
        self.assertNotIn("refusing", r.stderr)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def test_an_m1_results_file_still_uses_the_m1_baseline(self):
        r = self.run_gate(ROOT / "docs/perf/m70.json")
        self.assertIn("docs/perf/baseline.json", r.stdout)
        self.assertNotIn("baseline-m4", r.stdout)


if __name__ == "__main__":
    unittest.main()
