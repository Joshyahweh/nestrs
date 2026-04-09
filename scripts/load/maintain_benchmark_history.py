#!/usr/bin/env python3
import datetime as dt
import json
import pathlib
import re
import sys


TIMESTAMPED_JSON_RE = re.compile(r"^benchmark-report-(\d{8}-\d{6})\.json$")
TIMESTAMPED_MD_RE = re.compile(r"^benchmark-report-(\d{8}-\d{6})\.md$")


def parse_stamp(stamp: str) -> dt.datetime:
    return dt.datetime.strptime(stamp, "%Y%m%d-%H%M%S").replace(tzinfo=dt.timezone.utc)


def list_reports(reports_dir: pathlib.Path):
    json_reports = {}
    md_reports = {}
    for p in reports_dir.glob("benchmark-report-*"):
        m_json = TIMESTAMPED_JSON_RE.match(p.name)
        if m_json:
            json_reports[m_json.group(1)] = p
            continue
        m_md = TIMESTAMPED_MD_RE.match(p.name)
        if m_md:
            md_reports[m_md.group(1)] = p
    common = sorted(set(json_reports.keys()) & set(md_reports.keys()), reverse=True)
    return [(stamp, json_reports[stamp], md_reports[stamp]) for stamp in common]


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[2]
    reports_dir = root / "benchmarks" / "reports"
    reports_dir.mkdir(parents=True, exist_ok=True)
    history_path = reports_dir / "history.json"
    index_md_path = reports_dir / "INDEX.md"

    retention_count = int((root / "benchmarks" / "retention.count").read_text(encoding="utf-8").strip())
    retention_days = int((root / "benchmarks" / "retention.days").read_text(encoding="utf-8").strip())

    reports = list_reports(reports_dir)
    now = dt.datetime.now(dt.timezone.utc)
    cutoff = now - dt.timedelta(days=retention_days)

    kept = []
    pruned = []

    # Keep recent by count first.
    keep_stamps = {stamp for stamp, _, _ in reports[:retention_count]}
    for stamp, json_path, md_path in reports:
        stamp_dt = parse_stamp(stamp)
        keep = stamp in keep_stamps or stamp_dt >= cutoff
        if keep:
            kept.append((stamp, json_path, md_path))
        else:
            pruned.append((stamp, json_path, md_path))

    for stamp, j, m in pruned:
        j.unlink(missing_ok=True)
        m.unlink(missing_ok=True)
        print(f"[bench-history] pruned {stamp}")

    entries = []
    for stamp, json_path, md_path in kept:
        try:
            data = json.loads(json_path.read_text(encoding="utf-8"))
            entries.append(
                {
                    "stamp": stamp,
                    "generated_at_utc": data.get("generated_at_utc"),
                    "git_sha": data.get("git_sha"),
                    "json": json_path.name,
                    "markdown": md_path.name,
                }
            )
        except Exception:
            entries.append(
                {
                    "stamp": stamp,
                    "generated_at_utc": None,
                    "git_sha": None,
                    "json": json_path.name,
                    "markdown": md_path.name,
                }
            )

    history = {
        "retention": {
            "count": retention_count,
            "days": retention_days,
        },
        "entries": entries,
    }
    history_path.write_text(json.dumps(history, indent=2) + "\n", encoding="utf-8")

    lines = [
        "# Benchmark Report History",
        "",
        f"- Retention count: `{retention_count}`",
        f"- Retention days: `{retention_days}`",
        "",
        "| Stamp | Generated (UTC) | Commit | JSON | Markdown |",
        "|---|---|---|---|---|",
    ]
    for e in entries:
        lines.append(
            f"| `{e['stamp']}` | {e['generated_at_utc'] or '-'} | `{e['git_sha'] or '-'}` | `{e['json']}` | `{e['markdown']}` |"
        )
    lines.append("")
    index_md_path.write_text("\n".join(lines), encoding="utf-8")

    print(f"[bench-history] wrote {history_path} and {index_md_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
