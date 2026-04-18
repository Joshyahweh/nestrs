# ADR-0001: Axum-only HTTP engine

- Status: accepted
- Date: 2026-04-13

## Context

`nestrs` provides Nest-style module/controller ergonomics, but Rust web stacks differ in middleware and router semantics.
Supporting multiple HTTP engines in the core runtime increases abstraction overhead and review complexity.

## Decision

Core HTTP runtime remains Axum-only.

- `NestFactory` composes around Axum router/layer semantics.
- Public runtime APIs follow Axum/Tower request-response model.
- Alternative protocols (GraphQL, WS, microservices) layer on top of this core instead of introducing a second HTTP backend.

## Consequences

### Positive

- One execution model for middleware ordering, extractors, and response handling.
- Lower maintenance and clearer debugging path.
- Better parity between docs, tests, and production behavior.

### Negative

- No first-class adapter abstraction for other Rust HTTP frameworks.
- Integrations that assume another engine must bridge at app level.

## Contributor guidance

- Do not introduce engine-agnostic abstractions in core unless there is a concrete maintenance win and an approved follow-up ADR.
- Prefer Axum/Tower-native solutions in runtime code and examples.

## See also

- [HTTP pipeline order](../http-pipeline-order.md) — how Axum/Tower layers compose in nestrs.  
- [NestJS migration guide](../nestjs-migration.md) — middleware mapping from Nest.  
- Rustdoc: [`NestApplication::build_router`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html) (internal ordering comments in source when debugging).
