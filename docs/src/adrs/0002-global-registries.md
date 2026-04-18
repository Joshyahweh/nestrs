# ADR-0002: Process-global registries

- Status: accepted
- Date: 2026-04-13

## Context

The framework uses metadata generated at compile time (controllers, routes, decorators) and initialized at runtime.
Passing every registry through every API surface makes handler signatures and macro output noisy.

## Decision

Use process-global registries for framework metadata and selected runtime singletons.

- Metadata and route registries are initialized once per process.
- Test-only reset hooks are feature-gated (for deterministic integration tests).
- Public docs treat registry internals as stability-sensitive implementation details.

## Consequences

### Positive

- Simpler generated code and smaller handler signatures.
- Faster bootstrap paths and less repetitive state plumbing.
- Predictable lookup path for reflection-like behavior (`roles`, route metadata, etc.).

### Negative

- Requires care in tests to avoid cross-test leakage.
- Limits multi-tenant-in-single-process patterns that need isolated registries.

## Contributor guidance

- Any new global registry requires explicit test reset behavior (or rationale for why it is unnecessary).
- Changes that affect registry lifecycle must update `STABILITY.md` and related contributor docs.

## See also

- **`STABILITY.md`** at the repository root — semver, `test-hooks`, registry policy.  
- [Fundamentals](../fundamentals.md) — `MetadataRegistry`, route discovery, integration-test resets.  
- [Custom decorators](../custom-decorators.md) — `HandlerKey` and `#[roles]` metadata.
