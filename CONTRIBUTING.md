# Contributing to nestrs

Thanks for contributing to `nestrs`.

This guide keeps contributions consistent, reviewable, and safe for production hardening workflows.

## Development setup

### Prerequisites

- Rust stable toolchain
- Python 3 (for benchmark/report scripts)
- Optional: `k6` and `wrk` for load testing

### Initial setup

```bash
cargo check --workspace
cargo test --workspace
```

## Branch and commit conventions

- Use focused branches (feature/fix/docs/chore scope)
- Keep commits small and logically grouped
- Prefer clear messages that explain intent, not just file changes

Examples:

- `feat(core): add request-scoped metadata extractor`
- `fix(perf): stabilize relative regression baseline window`
- `docs(runbook): clarify storage sync workflow`

## Code style and quality gates

Before opening a PR, run:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

For performance-related changes, also run:

```bash
python3 scripts/load/check_benchmark_thresholds.py
python3 scripts/load/check_benchmark_relative_regression.py
```

## Pull request checklist

- [ ] Scope is clear and limited
- [ ] Tests were added/updated where behavior changed
- [ ] Docs were updated for any user-facing or operational change
- [ ] Security/performance implications were considered
- [ ] Benchmark-impacting changes include notes in the PR

## Documentation expectations

Update relevant docs when changing behavior:

- `README.md` for onboarding and quick start changes
- `PRODUCTION_RUNBOOK.md` for operational guidance
- `SECURITY.md` for security controls/assumptions
- `MICROSERVICES.md` for transport/event behavior
- `benchmarks/BASELINE.md` for benchmarking workflow changes

## Performance and benchmark contributions

If your change affects request handling, middleware, routing, or serialization:

1. Run benchmarks
2. Review regression gate output
3. Include a short before/after summary in your PR

Helpful scripts:

- `scripts/load/run_bench_ci.sh`
- `scripts/load/export_benchmark_report.py`
- `scripts/load/recommend_relative_thresholds.py`
- `scripts/load/evaluate_threshold_reassessment.py`

## Security contributions

- Do not commit secrets or private credentials
- Use OIDC and least-privilege patterns for cloud workflows
- Keep supply-chain checks green (`cargo audit` in CI)

## Reporting bugs

Please include:

- Environment (OS, Rust version)
- Reproduction steps
- Expected vs actual behavior
- Relevant logs/output
- If perf-related: benchmark output or report artifact

## Need help?

Open an issue with context, what you tried, and what you expected.

## Release process (maintainers)

1. Bump versions and update `CHANGELOG.md`.
2. Run local release gates (`fmt`, `clippy -D warnings`, `test`, `doc`, benchmark checks, publish dry-runs).
3. Create and push tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. Tag triggers `.github/workflows/publish-crates.yml` (preflight + publish using `CARGO_REGISTRY_TOKEN`, or trusted publishing if configured on crates.io).
5. If a bad release ships, use `cargo yank` and immediately cut a patch release.
