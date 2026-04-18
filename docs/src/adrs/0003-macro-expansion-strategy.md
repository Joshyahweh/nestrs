# ADR-0003: Macro expansion strategy

- Status: accepted
- Date: 2026-04-13

## Context

`nestrs` emulates Nest-style decorators through Rust proc macros.
Overly dynamic macro output can hide behavior and make compile errors difficult to trace.

## Decision

Macros favor explicit, predictable expansion with minimal hidden runtime behavior.

- Attribute macros emit straightforward Rust constructs and metadata registration calls.
- Runtime wiring is explicit in generated code paths (`impl_routes!`, module wiring, guard/pipe/interceptor attachment).
- Avoid macro magic that depends on non-local inference when explicit syntax is feasible.

## Consequences

### Positive

- Easier code review: generated behavior maps directly to source attributes.
- Better compile-time diagnostics and fewer runtime surprises.
- Lower maintenance risk for macro and runtime crates evolving together.

### Negative

- Some call sites are more verbose than highly inferred DSLs.
- Feature additions may require coordinated changes across macros and runtime docs.

## Contributor guidance

- New macro features must document expansion shape and failure modes.
- Prefer incremental, composable attributes over opaque one-shot mega-macros.
- If a macro feature increases hidden behavior, capture the trade-off in a new ADR.

## See also

- Crate: **`nestrs-macros`** — proc macro implementations and attribute reference.  
- [Custom decorators](../custom-decorators.md) — how attributes map to Nest-style patterns.  
- `impl_routes!` / `#[routes]` rustdoc on **`nestrs`** — ordering of guards, pipes, and interceptors.
