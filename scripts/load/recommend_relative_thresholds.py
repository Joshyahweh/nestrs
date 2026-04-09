#!/usr/bin/env python3
import json
import pathlib
import re
import statistics
import sys


REPORT_RE = re.compile(r"^benchmark-report-(\d{8}-\d{6})\.json$")


def load_json(path: pathlib.Path):
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def discover_reports_desc(reports_dir: pathlib.Path):
    reports = []
    for path in reports_dir.glob("benchmark-report-*.json"):
        m = REPORT_RE.match(path.name)
        if m:
            reports.append((m.group(1), path))
    reports.sort(key=lambda x: x[0], reverse=True)
    return [p for _, p in reports]


def collect_series(report_paths):
    series = {}
    for report_path in report_paths:
        report = load_json(report_path)
        for entry in report.get("entries", []):
            name = entry.get("name")
            value = entry.get("point_estimate_ns")
            if name is None or value is None:
                continue
            if value <= 0:
                continue
            series.setdefault(name, []).append(float(value))
    return series


def recommend_percent(values, sigma_multiplier, floor_pct):
    if len(values) < 2:
        return None
    mean = statistics.mean(values)
    if mean <= 0:
        return None
    std = statistics.pstdev(values)
    pct = (sigma_multiplier * std / mean) * 100.0
    return max(floor_pct, round(pct, 2))


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[2]
    reports_dir = root / "benchmarks" / "reports"
    thresholds_path = root / "benchmarks" / "relative_thresholds.json"
    out_path = root / "benchmarks" / "recommended_relative_thresholds.json"

    if not reports_dir.exists():
        print(f"[threshold-recommend] missing reports dir: {reports_dir}")
        return 1
    if not thresholds_path.exists():
        print(f"[threshold-recommend] missing current thresholds file: {thresholds_path}")
        return 1

    report_paths = discover_reports_desc(reports_dir)
    if len(report_paths) < 3:
        print("[threshold-recommend] need at least 3 timestamped reports for stable recommendation")
        return 0

    current = load_json(thresholds_path)
    series_by_bench = collect_series(report_paths)

    sigma_multiplier = 3.0
    floor_pct = 3.0
    output = {}
    lines = [
        "# Relative Threshold Recommendations",
        "",
        f"- Reports considered: {len(report_paths)}",
        f"- Rule: max( floor={floor_pct:.1f}%, {sigma_multiplier:.1f} * stddev / mean )",
        "",
        "| Benchmark | Current max % | Recommended max % | Samples |",
        "|---|---:|---:|---:|",
    ]

    for bench_name, cfg in current.items():
        values = series_by_bench.get(bench_name, [])
        recommended = recommend_percent(values, sigma_multiplier=sigma_multiplier, floor_pct=floor_pct)
        current_pct = cfg.get("max_regression_percent")
        samples = len(values)

        if recommended is None:
            rec_text = "-"
            output[bench_name] = dict(cfg)
        else:
            rec_text = f"{recommended:.2f}"
            updated = dict(cfg)
            updated["max_regression_percent"] = recommended
            output[bench_name] = updated

        lines.append(f"| `{bench_name}` | {current_pct} | {rec_text} | {samples} |")

    out_path.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
    md_path = root / "benchmarks" / "recommended_relative_thresholds.md"
    md_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    print(f"[threshold-recommend] wrote {out_path}")
    print(f"[threshold-recommend] wrote {md_path}")
    print("[threshold-recommend] review manually before applying")
    return 0


if __name__ == "__main__":
    sys.exit(main())
