#!/usr/bin/env python3
"""The performance gate: compares perf results with docs/perf/baseline.json.

    scripts/perf_gate.py docs/perf/m54.json
    scripts/perf_gate.py docs/perf/m62.json docs/perf/m62-*.json   (every row)
    scripts/perf_gate.py docs/perf/m54.json --update-baseline
    scripts/perf_gate.py docs/perf/m54.json --update-baseline --accept "why"

Several results files are checked one after the other (each against its
own row); the exit code is the worst. --update-baseline takes one file.

Exit code 0: every check passed. 1: a check failed. 2: the comparison was
refused (different machine, unknown row, unreadable file).

Rules (a row in baseline.json can override a ratio under "thresholds"):
  average_fps, one_percent_low_fps   at least 95% of baseline
  chunk_generation_ms, light_mesh_ms, every benchmark   at most 105%
  frames_over_50ms                   0
  memory_mib                         at most 110%
A row can also carry absolute "budgets", {"<measure>": {"min": x, "max": y}},
checked on the measures (or benchmarks) of the same name, so rows with no
baseline numbers (v1.5's new style rows) fit without changing this script.

--update-baseline rewrites the row only when every gated measure is equal or
better. --accept "<reason>" writes it anyway and records the reason: only use
it when Kenzo agreed in the milestone entry or in chat.
"""
import argparse
import datetime
import json
import sys
from pathlib import Path

HIGHER_IS_BETTER = {"average_fps", "one_percent_low_fps"}
# measure -> ("min_ratio" | "max_ratio", default)
RATIO_RULES = {
    "average_fps": ("min_ratio", 0.95),
    "one_percent_low_fps": ("min_ratio", 0.95),
    "chunk_generation_ms": ("max_ratio", 1.05),
    "light_mesh_ms": ("max_ratio", 1.05),
    "memory_mib": ("max_ratio", 1.10),
}
BENCH_RULE = ("max_ratio", 1.05)
ABSOLUTE_RULES = {"frames_over_50ms": 0}


def load(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError) as err:
        print(f"cannot read {path}: {err}", file=sys.stderr)
        sys.exit(2)


def check_row(results, row):
    """Returns table rows: (name, baseline, now, change, limit, ok)."""
    out = []
    thresholds = row.get("thresholds", {})
    measures = results.get("measures", {})
    benches = results.get("benchmarks", {})

    def ratio_check(name, base, now, rule):
        kind, default = rule
        limit = thresholds.get(name, {}).get(kind, default)
        if now is None:
            out.append((name, base, None, "missing", "", False))
            return
        ratio = now / base if base else float("inf") if now else 1.0
        ok = ratio >= limit if kind == "min_ratio" else ratio <= limit
        sign = ">=" if kind == "min_ratio" else "<="
        out.append((name, base, now, f"{(ratio - 1) * 100:+.1f}%", f"{sign} {limit * 100:.0f}%", ok))

    for name, rule in RATIO_RULES.items():
        if name in row.get("measures", {}):
            ratio_check(name, row["measures"][name], measures.get(name), rule)
    for name, base in sorted(row.get("benchmarks", {}).items()):
        ratio_check(f"bench: {name}", base, benches.get(name), BENCH_RULE)
    for name, limit in ABSOLUTE_RULES.items():
        if "measures" in row and row["measures"]:
            now = measures.get(name)
            out.append((name, limit, now, "", f"<= {limit}", now is not None and now <= limit))
    for name, bounds in sorted(row.get("budgets", {}).items()):
        now = measures.get(name, benches.get(name))
        ok = now is not None
        limits = []
        if ok and "min" in bounds:
            ok = ok and now >= bounds["min"]
        if ok and "max" in bounds:
            ok = ok and now <= bounds["max"]
        limits = " and ".join(f"{k} {v}" for k, v in bounds.items() if k in ("min", "max"))
        out.append((f"budget: {name}", "", now, "", limits, ok))
    return out


def equal_or_better(results, row):
    """True when every gated measure is at least as good as the baseline."""
    now_m, base_m = results.get("measures", {}), row.get("measures", {})
    for name in list(RATIO_RULES) + list(ABSOLUTE_RULES):
        if name not in base_m:
            continue
        now = now_m.get(name)
        if now is None:
            return False
        better = now >= base_m[name] if name in HIGHER_IS_BETTER else now <= base_m[name]
        if not better:
            return False
    for name, base in row.get("benchmarks", {}).items():
        now = results.get("benchmarks", {}).get(name)
        if now is None or now > base:
            return False
    return True


def fmt(value):
    if value is None:
        return "-"
    if value == "":
        return ""
    return f"{value:.2f}" if isinstance(value, float) else str(value)


def print_table(rows):
    print("| Measure | Baseline | Now | Change | Limit | Result |")
    print("|---|---|---|---|---|---|")
    for name, base, now, change, limit, ok in rows:
        print(f"| {name} | {fmt(base)} | {fmt(now)} | {change} | {limit} | {'pass' if ok else 'FAIL'} |")


def main():
    ap = argparse.ArgumentParser(description="Compare perf results with the baseline.")
    ap.add_argument("results", nargs="+")
    ap.add_argument("--baseline", default="docs/perf/baseline.json")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--accept", metavar="REASON")
    args = ap.parse_args()
    if args.accept and not args.update_baseline:
        ap.error("--accept goes with --update-baseline")
    if args.update_baseline and len(args.results) != 1:
        ap.error("--update-baseline takes one results file")
    if len(args.results) > 1:
        worst = 0
        for path in args.results:
            print(f"\n### {path}\n")
            worst = max(worst, gate(args, path))
        print("\nPASS (every file)" if worst == 0 else "\nFAIL (see above)")
        sys.exit(worst)
    sys.exit(gate(args, args.results[0]))


def gate(args, results_path):
    """Checks one results file; returns the exit code."""
    results = load(results_path)
    baseline_path = Path(args.baseline)
    baseline = load(baseline_path) if baseline_path.exists() else None
    row_name = results.get("row", "vanilla")

    if baseline is None:
        if not args.update_baseline:
            print(f"no baseline at {baseline_path}", file=sys.stderr)
            sys.exit(2)
        baseline = {"machine": results["machine"], "rows": {}}
    if baseline["machine"] != results.get("machine"):
        print(
            f"refusing to compare: baseline is from {baseline['machine']}, "
            f"these results are from {results.get('machine')}",
            file=sys.stderr,
        )
        sys.exit(2)
    row = baseline["rows"].get(row_name)
    if row is None and not args.update_baseline:
        print(f"the baseline has no row {row_name!r}", file=sys.stderr)
        sys.exit(2)

    if args.update_baseline:
        if row is None:
            row = {}
            print(f"new baseline row {row_name!r}")
        elif equal_or_better(results, row):
            print("every measure is equal or better: updating the baseline")
        elif args.accept:
            print(f"accepted by hand: {args.accept}")
        else:
            print_table(check_row(results, row))
            print("\nnot updating: some measure is worse. Use --accept only when Kenzo agreed.", file=sys.stderr)
            return 1
        new = {
            "commit": results.get("commit"),
            "recorded": datetime.date.today().isoformat(),
            "runs": results.get("runs"),
            "measures": results.get("measures", {}),
            "spread_pct": results.get("spread_pct", {}),
            "benchmarks": results.get("benchmarks", {}),
        }
        for kept in ("thresholds", "budgets", "accepted"):
            if kept in row:
                new[kept] = row[kept]
        if args.accept:
            new.setdefault("accepted", []).append(
                {"reason": args.accept, "commit": results.get("commit"), "date": new["recorded"]}
            )
        baseline["rows"][row_name] = new
        baseline_path.write_text(json.dumps(baseline, indent=2) + "\n")
        print(f"wrote {baseline_path}")
        return 0

    rows = check_row(results, row)
    print_table(rows)
    failed = [r[0] for r in rows if not r[5]]
    if failed:
        print(f"\nFAIL: {', '.join(failed)}")
        return 1
    print("\nPASS")
    return 0


if __name__ == "__main__":
    main()
