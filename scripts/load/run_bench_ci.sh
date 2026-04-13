#!/usr/bin/env bash
set -euo pipefail

# Stabilized Criterion parameters for CI comparability.
CRIT_ARGS=(--sample-size 20 --warm-up-time 3 --measurement-time 8 --noplot)

run_bench() {
  local bench_name="$1"
  if command -v taskset >/dev/null 2>&1; then
    # Pin to one core for lower variance in shared runners.
    taskset -c 0 cargo bench -p nestrs --bench "${bench_name}" -- "${CRIT_ARGS[@]}"
  else
    cargo bench -p nestrs --bench "${bench_name}" -- "${CRIT_ARGS[@]}"
  fi
}

echo "[bench-ci] running router_hot_path"
run_bench router_hot_path

echo "[bench-ci] running router_middleware_stack"
run_bench router_middleware_stack

echo "[bench-ci] running di_resolution"
run_bench di_resolution

echo "[bench-ci] running json_validation_hot_path"
run_bench json_validation_hot_path

echo "[bench-ci] checking thresholds"
python3 scripts/load/check_benchmark_thresholds.py
