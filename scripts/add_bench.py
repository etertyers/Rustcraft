#!/usr/bin/env python3
"""Adds the median of each criterion benchmark (in ms) to a perf results JSON.

    cargo bench
    scripts/add_bench.py docs/perf/m53.json [target/criterion]

Reads <criterion dir>/*/new/estimates.json (median.point_estimate, in ns) and
the benchmark's name from the benchmark.json beside it, and stores them under
"benchmarks" in the results file.
"""
import json
import sys
from pathlib import Path


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    results = Path(sys.argv[1])
    root = Path(sys.argv[2] if len(sys.argv) == 3 else "target/criterion")
    found = {}
    for estimates in sorted(root.glob("*/new/estimates.json")):
        name = json.loads((estimates.parent / "benchmark.json").read_text())["full_id"]
        found[name] = json.loads(estimates.read_text())["median"]["point_estimate"] / 1e6
    if not found:
        sys.exit(f"no benchmarks under {root}: run cargo bench first")
    data = json.loads(results.read_text())
    data["benchmarks"] = found
    results.write_text(json.dumps(data, indent=2) + "\n")
    for name, ms in found.items():
        print(f"{name}: {ms:.4f} ms")


if __name__ == "__main__":
    main()
