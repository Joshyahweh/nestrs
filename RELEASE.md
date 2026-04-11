# Release Guide

This guide describes how to prepare and publish a new `nestrs` release.

## Release goals

- Ship stable, documented, reproducible versions
- Keep CI/security/performance gates green
- Publish clear change notes for users

## Pre-release checklist

### 1) Sync and verify branch state

- Ensure release branch is up to date
- Ensure no unintended local changes are included

### 2) Run quality gates

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

### 3) Run benchmark gates

```bash
python3 scripts/load/check_benchmark_thresholds.py
python3 scripts/load/check_benchmark_relative_regression.py
python3 scripts/load/evaluate_threshold_reassessment.py
```

If thresholds require retuning, document rationale and update via reviewed recommendation outputs.

### 4) Dry-run publish validation

```bash
cargo publish -p nestrs-macros --dry-run --locked
cargo publish -p nestrs-core --dry-run --locked
cargo publish -p nestrs-microservices --dry-run --locked
cargo publish -p nestrs-ws --dry-run --locked
cargo publish -p nestrs-graphql --dry-run --locked
cargo publish -p nestrs-openapi --dry-run --locked
cargo publish -p nestrs --dry-run --locked
cargo publish -p nestrs-prisma --dry-run --locked
cargo publish -p nestrs-scaffold --dry-run --locked
```

### 5) Confirm docs are current

Review and update as needed:

- `README.md`
- `PRODUCTION_RUNBOOK.md`
- `SECURITY.md`
- `MICROSERVICES.md`

## Versioning

Use semantic versioning:

- **MAJOR**: breaking API changes
- **MINOR**: backward-compatible features
- **PATCH**: backward-compatible fixes/docs/infra changes

Update crate versions consistently where required by workspace policy.

Release metadata consistency is enforced in CI by:

- `.github/workflows/release-version-check.yml`
- `scripts/release/check_version_sync.py`

Rule: `VERSION` must match the latest released heading in `CHANGELOG.md` (and `Unreleased` must exist).

## Changelog and notes

Prepare release notes with:

- Highlights (feature/fix/perf/security)
- Breaking changes (if any)
- Migration notes
- Known limitations

## Tagging and publishing (example flow)

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

### First publish (manual, required once)

The first release should be manual from a maintainer machine. crates.io versions are immutable.

Recommended initial order for this workspace:

```bash
cargo publish -p nestrs-macros
sleep 30
cargo publish -p nestrs-core
sleep 30
cargo publish -p nestrs-events
sleep 30
cargo publish -p nestrs-ws
sleep 30
cargo publish -p nestrs-graphql
sleep 30
cargo publish -p nestrs-openapi
sleep 30
cargo publish -p nestrs-cqrs
sleep 30
cargo publish -p nestrs-microservices
sleep 30
cargo publish -p nestrs
sleep 30
cargo publish -p nestrs-prisma
sleep 30
cargo publish -p nestrs-scaffold
```

### crates.io authentication (GitHub Actions)

The `publish-crates` workflow expects a repository secret **`CARGO_REGISTRY_TOKEN`** (a crates.io API token with publish scope). The publish job runs only after the workflow’s **preflight** job (`fmt`, `clippy`, `test`, `cargo audit`) succeeds.

### Optional: Trusted Publishing (OIDC)

If you configure this repository as a trusted publisher on crates.io for each crate, you can switch the workflow back to `rust-lang/crates-io-auth-action` and drop the long-lived token. Until that is configured, use `CARGO_REGISTRY_TOKEN` as above.

## Post-release checks

- Verify release tag and notes are visible
- Verify CI workflows are healthy on tagged commit
- Validate docs links and artifact references
- Confirm benchmark report artifacts are generated as expected

## Rollback/mitigation plan

If critical regression is found:

1. Open incident issue immediately
2. Reproduce and scope impact
3. Ship patch release (`X.Y.(Z+1)`) with fix
4. Document issue and mitigation in release notes

If a release is broken but must remain in history, use `cargo yank --vers <version> <crate>` to stop new consumers from selecting it.
