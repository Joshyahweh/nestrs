#!/usr/bin/env python3
import csv
import json
import pathlib
import sys


def load_json(path: pathlib.Path):
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[2]
    reports_dir = root / "benchmarks" / "reports"
    history_path = reports_dir / "history.json"
    if not history_path.exists():
        print(f"[bench-dashboard] missing history file: {history_path}")
        return 1

    history = load_json(history_path)
    entries = history.get("entries", [])
    # Oldest-first for chart rendering.
    entries = list(reversed(entries))

    points = []
    for e in entries:
        json_file = reports_dir / e["json"]
        if not json_file.exists():
            continue
        data = load_json(json_file)
        generated_at = data.get("generated_at_utc")
        git_sha = data.get("git_sha")
        for item in data.get("entries", []):
            points.append(
                {
                    "generated_at_utc": generated_at,
                    "git_sha": git_sha,
                    "benchmark": item.get("name"),
                    "status": item.get("status"),
                    "point_estimate_ns": item.get("point_estimate_ns"),
                    "threshold_ns": item.get("threshold_ns"),
                }
            )

    timeseries_json = reports_dir / "timeseries.json"
    timeseries_json.write_text(json.dumps({"points": points}, indent=2) + "\n", encoding="utf-8")

    timeseries_csv = reports_dir / "timeseries.csv"
    with timeseries_csv.open("w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(
            f,
            fieldnames=[
                "generated_at_utc",
                "git_sha",
                "benchmark",
                "status",
                "point_estimate_ns",
                "threshold_ns",
            ],
        )
        writer.writeheader()
        for p in points:
            writer.writerow(p)

    dashboard_html = reports_dir / "dashboard.html"
    dashboard_html.write_text(
        """<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Benchmark Trend Dashboard</title>
  <style>
    body { font-family: system-ui, sans-serif; margin: 20px; }
    h1 { margin-bottom: 4px; }
    .meta { color: #555; margin-bottom: 16px; }
    table { border-collapse: collapse; width: 100%; }
    th, td { border: 1px solid #ddd; padding: 8px; font-size: 14px; }
    th { background: #f5f5f5; text-align: left; }
    .pass { color: #0a7f2e; font-weight: 600; }
    .fail, .missing { color: #b42318; font-weight: 600; }
    .mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  </style>
</head>
<body>
  <h1>Benchmark Trend Dashboard</h1>
  <div class="meta">Data source: <code>timeseries.json</code></div>
  <table id="tbl">
    <thead>
      <tr>
        <th>Generated (UTC)</th>
        <th>Commit</th>
        <th>Benchmark</th>
        <th>Mean point estimate (ns)</th>
        <th>Threshold (ns)</th>
        <th>Status</th>
      </tr>
    </thead>
    <tbody></tbody>
  </table>
  <script>
    fetch('timeseries.json')
      .then(r => r.json())
      .then(({ points }) => {
        const tbody = document.querySelector('#tbl tbody');
        for (const p of points) {
          const tr = document.createElement('tr');
          tr.innerHTML = `
            <td class="mono">${p.generated_at_utc ?? '-'}</td>
            <td class="mono">${p.git_sha ?? '-'}</td>
            <td class="mono">${p.benchmark ?? '-'}</td>
            <td>${p.point_estimate_ns ?? '-'}</td>
            <td>${p.threshold_ns ?? '-'}</td>
            <td class="${p.status ?? 'missing'}">${p.status ?? 'missing'}</td>
          `;
          tbody.appendChild(tr);
        }
      })
      .catch((err) => {
        const tbody = document.querySelector('#tbl tbody');
        const tr = document.createElement('tr');
        tr.innerHTML = `<td colspan="6">Failed to load timeseries.json: ${String(err)}</td>`;
        tbody.appendChild(tr);
      });
  </script>
</body>
</html>
""",
        encoding="utf-8",
    )

    print(f"[bench-dashboard] wrote {timeseries_json}, {timeseries_csv}, {dashboard_html}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
