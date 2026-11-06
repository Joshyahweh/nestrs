#!/usr/bin/env bash
# Wave 5 — compile + smoke gate for nestrs-resilience policy overhead.
#
# Full Criterion samples can SIGSEGV on some runners under `--quick` / plotters;
# CI asserts the bench **target builds**, then runs a short unit smoke that
# exercises the same execute path without Criterion.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

cargo check -p nestrs-resilience --benches --quiet
cargo test -p nestrs-resilience --lib service::tests -- --nocapture

echo "resilience_overhead: ok"
