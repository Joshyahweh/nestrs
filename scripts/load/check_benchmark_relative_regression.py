#!/usr/bin/env python3
import json
import pathlib
import re
import sys


REPORT_RE = re.compile(r"^benchmark-report-(\d{8}-\d{6})\.json$")


def load_json(path: pathlib.Path):
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def entry_map(report: dict) -> dict:
    out = {}
    for entry in report.get("entries", []):
        name = entry.get("name")
        point = entry.get("point_estimate_ns")
        if name is not None and point is not None:
            out[name] = float(point)
    return out


def list_report_paths_desc(reports_dir: pathlib.Path):
    stamped = []
    for p in reports_dir.glob("benchmark-report-*.json"):
        m = REPORT_RE.match(p.name)
        if m:
            stamped.append((m.group(1), p))
    stamped.sort(key=lambda x: x[0], reverse=True)
    return [p for _, p in stamped]


def median(values):
    ordered = sorted(values)
    n = len(ordered)
    if n == 0:
        return None
    mid = n // 2
    if n % 2 == 1:
        return ordered[mid]
    return (ordered[mid - 1] + ordered[mid]) / 2.0


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[2]
    reports_dir = root / "benchmarks" / "reports"
    thresholds_path = root / "benchmarks" / "relative_thresholds.json"

    if not reports_dir.exists():
        print("[perf-relative-gate] reports directory missing; skipping")
        return 0
    if not thresholds_path.exists():
        print(f"[perf-relative-gate] missing thresholds file: {thresholds_path}")
        return 1

    report_paths = list_report_paths_desc(reports_dir)
    if len(report_paths) < 2:
        print("[perf-relative-gate] no prior timestamped benchmark report found; skipping")
        return 0

    thresholds = load_json(thresholds_path)
    current = load_json(report_paths[0])
    current_points = entry_map(current)

    failures = []
    checked = 0

    for bench_name, cfg in thresholds.items():
        max_regression_pct = cfg.get("max_regression_percent")
        if max_regression_pct is None:
            failures.append(f"{bench_name}: missing max_regression_percent in thresholds")
            continue

        baseline_window = int(cfg.get("baseline_window", 3))
        min_baseline_runs = int(cfg.get("min_baseline_runs", 2))
        if baseline_window < 1:
            failures.append(f"{bench_name}: baseline_window must be >= 1")
            continue
        if min_baseline_runs < 1:
            failures.append(f"{bench_name}: min_baseline_runs must be >= 1")
            continue

        cur = current_points.get(bench_name)
        if cur is None:
            print(f"[perf-relative-gate] {bench_name}: missing in current report; skipping benchmark")
            continue

        baseline_values = []
        for baseline_path in report_paths[1 : baseline_window + 1]:
            baseline_points = entry_map(load_json(baseline_path))
            value = baseline_points.get(bench_name)
            if value is not None and value > 0:
                baseline_values.append(value)

        if len(baseline_values) < min_baseline_runs:
            print(
                f"[perf-relative-gate] {bench_name}: only {len(baseline_values)} baseline runs "
                f"(requires {min_baseline_runs}); skipping benchmark"
            )
            continue

        baseline = median(baseline_values)
        if baseline is None or baseline <= 0:
            failures.append(f"{bench_name}: computed baseline must be > 0 (got {baseline})")
            continue

        checked += 1
        regression_pct = ((cur - baseline) / baseline) * 100.0
        baseline_desc = f"median(last {len(baseline_values)} runs)"
        print(
            f"[perf-relative-gate] {bench_name}: current={cur:.0f}ns {baseline_desc}={baseline:.0f}ns "
            f"delta={regression_pct:+.2f}% max={max_regression_pct:.2f}%"
        )
        if regression_pct > float(max_regression_pct):
            failures.append(
                f"{bench_name}: regression {regression_pct:+.2f}% vs {baseline_desc} exceeded +{max_regression_pct:.2f}%"
            )

    if checked == 0:
        print("[perf-relative-gate] no benchmarks were comparable; skipping")
        return 0

    if failures:
        print("[perf-relative-gate] regression failures:")
        for f in failures:
            print(f"  - {f}")
        return 1

    print("[perf-relative-gate] all relative regression checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
