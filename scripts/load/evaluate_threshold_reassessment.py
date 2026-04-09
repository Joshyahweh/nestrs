#!/usr/bin/env python3
import json
import pathlib
import re
import sys


REPORT_RE = re.compile(r"^benchmark-report-(\d{8}-\d{6})\.json$")


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[2]
    reports_dir = root / "benchmarks" / "reports"
    out_json = root / "benchmarks" / "threshold_reassessment_status.json"
    out_md = root / "benchmarks" / "threshold_reassessment_status.md"

    required_reports = 3
    found_reports = 0
    report_names = []

    if reports_dir.exists():
        for path in reports_dir.glob("benchmark-report-*.json"):
            if REPORT_RE.match(path.name):
                found_reports += 1
                report_names.append(path.name)

    report_names.sort(reverse=True)
    ready = found_reports >= required_reports
    remaining = max(0, required_reports - found_reports)

    payload = {
        "required_reports": required_reports,
        "found_reports": found_reports,
        "ready_for_reassessment": ready,
        "remaining_reports_needed": remaining,
        "latest_reports": report_names[:5],
        "next_action": (
            "Threshold reassessment cycle is complete for current history; continue periodic re-evaluation on new scheduled runs."
            if ready
            else "Wait for additional scheduled benchmark runs, then re-evaluate."
        ),
    }
    out_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    md_lines = [
        "# Threshold Reassessment Status",
        "",
        f"- Required timestamped benchmark reports: `{required_reports}`",
        f"- Found reports: `{found_reports}`",
        f"- Ready for reassessment: `{'yes' if ready else 'no'}`",
        f"- Remaining reports needed: `{remaining}`",
        "",
        "## Recent reports",
        "",
    ]
    if report_names:
        for name in report_names[:5]:
            md_lines.append(f"- `{name}`")
    else:
        md_lines.append("- none")
    md_lines.extend(
        [
            "",
            "## Next action",
            "",
            payload["next_action"],
            "",
            "When ready, run:",
            "",
            "```bash",
            "python3 scripts/load/recommend_relative_thresholds.py",
            "```",
            "",
        ]
    )
    out_md.write_text("\n".join(md_lines), encoding="utf-8")

    print(f"[threshold-reassess] wrote {out_json}")
    print(f"[threshold-reassess] wrote {out_md}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
