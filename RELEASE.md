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
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

### 3) Run benchmark gates

```bash
python3 scripts/load/check_benchmark_thresholds.py
python3 scripts/load/check_benchmark_relative_regression.py
python3 scripts/load/evaluate_threshold_reassessment.py
```

If thresholds require retuning, document rationale and update via reviewed recommendation outputs.

### 4) Confirm docs are current

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

If publishing crates:

```bash
cargo publish -p nestrs-core
cargo publish -p nestrs-macros
cargo publish -p nestrs
```

Publish order may vary by dependency graph; publish foundational crates first.

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
