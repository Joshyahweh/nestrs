#!/usr/bin/env python3
import json
import pathlib
import sys


def load_json(path: pathlib.Path):
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[2]
    thresholds_path = root / "benchmarks" / "thresholds.json"
    if not thresholds_path.exists():
        print(f"[perf-gate] missing thresholds file: {thresholds_path}")
        return 1

    thresholds = load_json(thresholds_path)
    criterion_root = root / "target" / "criterion"
    if not criterion_root.exists():
        print("[perf-gate] no criterion output found at target/criterion")
        return 1

    failures = []
    checked = 0

    for bench_name, cfg in thresholds.items():
        estimates_path = criterion_root / bench_name / "new" / "estimates.json"
        if not estimates_path.exists():
            failures.append(f"{bench_name}: missing estimates file ({estimates_path})")
            continue

        estimates = load_json(estimates_path)
        point_estimate = estimates.get("mean", {}).get("point_estimate")
        if point_estimate is None:
            failures.append(f"{bench_name}: malformed estimates.json (missing mean.point_estimate)")
            continue

        max_ns = cfg.get("max_point_estimate_ns")
        if max_ns is None:
            failures.append(f"{bench_name}: missing max_point_estimate_ns in thresholds")
            continue

        checked += 1
        print(f"[perf-gate] {bench_name}: mean.point_estimate={point_estimate:.0f}ns threshold={max_ns}ns")
        if point_estimate > max_ns:
            failures.append(
                f"{bench_name}: point_estimate {point_estimate:.0f}ns exceeded threshold {max_ns}ns"
            )

    if checked == 0:
        print("[perf-gate] no benchmarks checked")
        return 1

    if failures:
        print("[perf-gate] threshold failures:")
        for f in failures:
            print(f"  - {f}")
        return 1

    print("[perf-gate] all configured thresholds passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
