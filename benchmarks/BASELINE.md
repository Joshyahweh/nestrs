# Benchmark Baseline

Track benchmark and load-test snapshots over time.

## How to run

```bash
# Criterion microbenchmarks
cargo bench -p nestrs --bench router_hot_path
cargo bench -p nestrs --bench router_middleware_stack

# End-to-end load profiles (requires running app)
chmod +x scripts/load/run_profiles.sh
BASE_URL=http://127.0.0.1:3000 scripts/load/run_profiles.sh
```

## Latest baseline (fill after each run)

| Date | Commit | router_hot_path (ns/iter) | router_middleware_stack (ns/iter) | wrk p95 | wrk p99 | k6 p95 | k6 p99 | Error rate |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| YYYY-MM-DD | <sha> | - | - | - | - | - | - | - |

## Notes

- Use same machine class/runtime settings for meaningful comparisons.
- Compare with and without backpressure (`use_concurrency_limit` + `use_load_shed`).
- Investigate regressions before release if p95/p99 or error rate significantly increases.

## Regression thresholds

- Threshold file: `benchmarks/thresholds.json`
- Gate script: `scripts/load/check_benchmark_thresholds.py`
- Relative threshold file: `benchmarks/relative_thresholds.json`
- Relative gate script: `scripts/load/check_benchmark_relative_regression.py`
- Relative threshold recommendation script: `scripts/load/recommend_relative_thresholds.py`
- Report export: `scripts/load/export_benchmark_report.py`

Relative gate behavior:

- compares current run against median of recent prior runs
- baseline window and required history are configurable per benchmark in `benchmarks/relative_thresholds.json`
- skips only when there is insufficient historical data

Run manually after benches:

```bash
python3 scripts/load/check_benchmark_thresholds.py
python3 scripts/load/export_benchmark_report.py
python3 scripts/load/check_benchmark_relative_regression.py
python3 scripts/load/recommend_relative_thresholds.py
python3 scripts/load/evaluate_threshold_reassessment.py
```

Recommendation outputs:

- `benchmarks/recommended_relative_thresholds.json`
- `benchmarks/recommended_relative_thresholds.md`
- `benchmarks/threshold_reassessment_status.json`
- `benchmarks/threshold_reassessment_status.md`

Generated report files:

- `benchmarks/reports/latest.json`
- `benchmarks/reports/latest.md`
- `benchmarks/reports/history.json`
- `benchmarks/reports/INDEX.md`
- `benchmarks/reports/timeseries.json`
- `benchmarks/reports/timeseries.csv`
- `benchmarks/reports/dashboard.html`

Optional PR comment bot:

- Script: `scripts/load/post_pr_benchmark_comment.py`
- Triggered by `.github/workflows/performance.yml` on pull requests.

History/retention maintenance:

- Script: `scripts/load/maintain_benchmark_history.py`
- Retention count: `benchmarks/retention.count`
- Retention days: `benchmarks/retention.days`

Dashboard build:

- Script: `scripts/load/build_benchmark_dashboard.py`
- Produces static trend artifacts suitable for CI artifact download or static hosting.

External publishing:

- Workflow: `.github/workflows/performance.yml`
- Optional GitHub Pages publish (`publish_dashboard=true` on `workflow_dispatch`, `main` branch).
- Publishable artifact bundle: `benchmark-dashboard-publish` (for S3/GCS/Azure upload).
- Scheduled trend capture: workflow cron runs weekly to keep benchmark history current.
- Long-term storage and restore runbook: `BENCHMARK_STORAGE_PLAYBOOK.md`.
- Storage sync workflow template: `.github/workflows/benchmark-storage-sync.yml`.
- Provider secret/bootstrap checklist: `BENCHMARK_STORAGE_SECRETS_CHECKLIST.md`.

Threshold recommendation helper outputs:

- `benchmarks/recommended_relative_thresholds.json`
- `benchmarks/recommended_relative_thresholds.md`
- Optional closeout summary: `PHASE5_OPTIONAL_CLOSEOUT.md`
