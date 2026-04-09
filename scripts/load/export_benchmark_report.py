#!/usr/bin/env python3
import datetime as dt
import json
import pathlib
import subprocess
import sys


def git_sha(root: pathlib.Path) -> str:
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=str(root),
            stderr=subprocess.DEVNULL,
            text=True,
        )
        return out.strip()
    except Exception:
        return "unknown"


def load_json(path: pathlib.Path):
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[2]
    thresholds_path = root / "benchmarks" / "thresholds.json"
    criterion_root = root / "target" / "criterion"
    out_dir = root / "benchmarks" / "reports"
    out_dir.mkdir(parents=True, exist_ok=True)

    if not thresholds_path.exists():
        print(f"[bench-report] missing thresholds file: {thresholds_path}")
        return 1
    if not criterion_root.exists():
        print(f"[bench-report] missing criterion output directory: {criterion_root}")
        return 1

    thresholds = load_json(thresholds_path)
    now_utc = dt.datetime.now(dt.timezone.utc)
    stamp = now_utc.strftime("%Y%m%d-%H%M%S")
    sha = git_sha(root)

    entries = []
    for bench_name, cfg in thresholds.items():
        estimates_path = criterion_root / bench_name / "new" / "estimates.json"
        if not estimates_path.exists():
            entries.append(
                {
                    "name": bench_name,
                    "status": "missing",
                    "threshold_ns": cfg.get("max_point_estimate_ns"),
                }
            )
            continue

        estimates = load_json(estimates_path)
        point_estimate = estimates.get("mean", {}).get("point_estimate")
        threshold = cfg.get("max_point_estimate_ns")
        status = (
            "pass"
            if (point_estimate is not None and threshold is not None and point_estimate <= threshold)
            else "fail"
        )
        entries.append(
            {
                "name": bench_name,
                "status": status,
                "point_estimate_ns": point_estimate,
                "threshold_ns": threshold,
            }
        )

    report = {
        "generated_at_utc": now_utc.isoformat(),
        "git_sha": sha,
        "entries": entries,
    }

    json_path = out_dir / "latest.json"
    with json_path.open("w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)
        f.write("\n")

    timestamped_json = out_dir / f"benchmark-report-{stamp}.json"
    with timestamped_json.open("w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)
        f.write("\n")

    md_lines = [
        "# Benchmark Report",
        "",
        f"- Generated: {report['generated_at_utc']}",
        f"- Commit: `{sha}`",
        "",
        "| Benchmark | Mean point estimate (ns) | Threshold (ns) | Status |",
        "|---|---:|---:|---|",
    ]
    for e in entries:
        pe = e.get("point_estimate_ns")
        pe_s = "-" if pe is None else f"{pe:.0f}"
        th = e.get("threshold_ns")
        th_s = "-" if th is None else str(th)
        md_lines.append(f"| `{e['name']}` | {pe_s} | {th_s} | {e['status']} |")
    md_lines.append("")

    md_path = out_dir / "latest.md"
    md_path.write_text("\n".join(md_lines), encoding="utf-8")

    timestamped_md = out_dir / f"benchmark-report-{stamp}.md"
    timestamped_md.write_text("\n".join(md_lines), encoding="utf-8")

    print(f"[bench-report] wrote {json_path} and {md_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
