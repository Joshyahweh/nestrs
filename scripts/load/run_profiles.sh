#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"

echo "[perf] BASE_URL=${BASE_URL}"
echo "[perf] Running wrk baseline checks..."
wrk -t4 -c64 -d30s "${BASE_URL}/api/v1/api"
wrk -t4 -c64 -d30s "${BASE_URL}/health"

echo "[perf] Running k6 smoke/burst profile..."
k6 run scripts/load/k6-api-smoke.js

echo "[perf] Done. Record p50/p95/p99/error-rate in benchmarks/BASELINE.md"
