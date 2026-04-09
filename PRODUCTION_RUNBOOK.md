# nestrs Production Runbook

This runbook captures deployment and operations guidance for nestrs applications.

## Environment and prod flags

- `NESTRS_ENV=production` enables production-safe behavior when your app uses:
  - `enable_production_errors_from_env()`
- `RUST_LOG` (or `NESTRS_LOG`) controls tracing filter directives.
- `PORT` controls bind port when your app reads env at startup.

## Release build profile

Recommended `Cargo.toml` release profile:

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

## Container deployment

Use multi-stage Docker builds:

1. Build with `cargo build --release`.
2. Copy only the release binary to a slim runtime image.
3. Set `NESTRS_ENV=production`.
4. Expose app + probe endpoints.

## Observability baseline

- Use `use_request_id()` for correlation IDs.
- Use `use_request_tracing(...)` for request lifecycle logs.
- Use `enable_metrics("/metrics")` for Prometheus scrape endpoint.
- Use `enable_health_check("/health")` and readiness checks for probes.

## Load and performance guidance

- Enable backpressure controls:
  - `use_concurrency_limit(...)` to cap in-flight requests.
  - `use_load_shed()` to reject overload quickly with `503` instead of queue growth.
- Keep timeout/body-limit/rate-limit policies aligned with expected traffic profile.
- Validate service behavior under load before production rollout.
- Prefer staged load tests (baseline, steady-state, burst, failover scenarios).

### Benchmark scaffolding

The `nestrs` crate includes a Criterion benchmark harness:

```bash
cargo bench -p nestrs --bench router_hot_path
cargo bench -p nestrs --bench router_middleware_stack
```

Use these benchmarks as baselines when evaluating route/middleware performance changes.

### External load testing

For end-to-end throughput and latency:

- `wrk` for quick HTTP stress checks
- `k6` for scripted scenarios (auth flows, mixed routes, sustained load)
- compare p50/p95/p99 latency, error rate, and saturation metrics

#### `wrk` quick checks

```bash
wrk -t4 -c64 -d30s http://127.0.0.1:3000/api/v1/api
wrk -t4 -c64 -d30s http://127.0.0.1:3000/health
```

#### `k6` scripted smoke/burst suite

Script location:

- `scripts/load/k6-api-smoke.js`
- `scripts/load/run_profiles.sh`

Run:

```bash
k6 run scripts/load/k6-api-smoke.js
BASE_URL=http://127.0.0.1:3000 k6 run scripts/load/k6-api-smoke.js
chmod +x scripts/load/run_profiles.sh
BASE_URL=http://127.0.0.1:3000 scripts/load/run_profiles.sh
```

Suggested scenario progression:

1. Baseline (`10` VUs steady).
2. Burst ramp (`25` → `50` VUs).
3. Repeat with backpressure enabled (`use_concurrency_limit` + `use_load_shed`).

### CI and baseline tracking

- CI workflow: `.github/workflows/performance.yml`
  - compiles benches
  - runs stabilized criterion benches (`scripts/load/run_bench_ci.sh`)
  - enforces benchmark regression thresholds
  - enforces relative regression checks using rolling baseline median from prior reports
  - exports benchmark report (`benchmarks/reports/latest.{json,md}`)
  - maintains benchmark history index + retention pruning
  - builds trend dashboard artifacts (`dashboard.html`, timeseries files)
  - publishes markdown summary in GitHub Actions step summary
  - optionally posts/updates a benchmark report comment on PRs
  - optionally publishes dashboard bundle to GitHub Pages (`/perf`)
  - uploads `target/criterion` as artifact
- Record key snapshots in `benchmarks/BASELINE.md`.
- Tune gate values in `benchmarks/thresholds.json`.
- Retention policy files:
  - `benchmarks/retention.count`
  - `benchmarks/retention.days`
  - Relative gate config: `benchmarks/relative_thresholds.json` (`max_regression_percent`, `baseline_window`, `min_baseline_runs`)
  - Threshold recommendation helper: `scripts/load/recommend_relative_thresholds.py`

Scheduled benchmark execution:

- `.github/workflows/performance.yml` also runs on a weekly cron (`0 3 * * 1`) to provide periodic trend snapshots.

### External dashboard publishing (optional)

GitHub Pages path:

- Run `performance` workflow via `workflow_dispatch` with:
  - `publish_dashboard=true`
- On `main`, workflow publishes benchmark dashboard to `gh-pages` under `perf/`.

Object storage path (S3/GCS/Azure Blob):

- Use uploaded artifact `benchmark-dashboard-publish` from the workflow.
- Sync bundle contents to your static hosting bucket/container.
- Serve `index.html` + `timeseries.json` as static assets.
- Use `BENCHMARK_STORAGE_PLAYBOOK.md` for long-term object storage layout, retention, and restore workflow.
- Use `.github/workflows/benchmark-storage-sync.yml` as a manual-dispatch template to sync `benchmarks/reports` to S3/GCS/Azure.
- Complete provider bootstrap first via `BENCHMARK_STORAGE_SECRETS_CHECKLIST.md` (OIDC + least-privilege setup).

Optional threshold tuning artifacts (generated locally; paths are gitignored and not part of public clones):

- Recommendation helper outputs:
  - `benchmarks/recommended_relative_thresholds.json`
  - `benchmarks/recommended_relative_thresholds.md`
  - `benchmarks/threshold_reassessment_status.json`
  - `benchmarks/threshold_reassessment_status.md`
- Optional closeout summary and artifact index:
  - `PHASE5_OPTIONAL_CLOSEOUT.md`

Stability knobs used in CI:

- pinned CPU core when available (`taskset -c 0`)
- fixed criterion args (`--sample-size 20 --warm-up-time 3 --measurement-time 8 --noplot`)
- normalized `RUSTFLAGS=-C target-cpu=x86-64-v2`

## Panic and blocking guidance

- Prefer `panic = "abort"` in production builds with supervisor restarts.
- Use `tokio::task::spawn_blocking` for blocking IO or CPU-heavy work.
- Avoid synchronous sleeps in async contexts (`std::thread::sleep`).

## Database migrations/seeding

Before rolling out new app versions:

1. Run schema migrations.
2. Run seed jobs if required.
3. Deploy application binary.

For Prisma-based apps, run migration/seed commands in CI/CD or a pre-deploy job.

## Connection pools

- Tune DB pools per environment and expected concurrency.
- Start with conservative defaults and load-test before increasing limits.
- Monitor saturation/error rates and adjust.

## Deployment parity themes

Map Nest-like concerns to nestrs equivalents:

- Health endpoint and readiness checks.
- Structured logs with correlation IDs.
- Metrics scrape endpoint.
- Environment-driven production error behavior.
