# HTTP pipeline ordering

This page documents **deterministic** ordering for cross-cutting concerns. When in doubt, prefer the **tests** in `nestrs/tests/cross_cutting_ordering_contract.rs` and the **`impl_routes!` rustdoc** on `nestrs::impl_routes` (source: `nestrs/src/lib.rs`).

**Examples:** A minimal `#[routes]` sample with **`#[use_guards]`**, **`#[use_interceptors]`**, and **`#[use_filters]`** is in the [API cookbook](appendix-api-cookbook.md). For **`use_global_layer`** / **`use_global_exception_filter`**, see the same chapter.

## Request flow (conceptual)

Incoming HTTP requests hit **global** `NestApplication::build_router` layers first (CORS, security headers, rate limits, request id, optional CSRF, and anything you add with `use_global_layer`), then the **per-route** stack below for the matched handler.

For **one route**, think of the pipeline as moving **inward** through filters → guards → interceptors → **handler**, then **outward** through interceptors and filters again (filters handle `HttpException` mapping on the error path):

```text
Client
  → [global middleware stack]
  → [exception filters (outer … inner)]
  → [controller guard, if any]
  → [route guards: G1, G2, …]
  → [interceptors (outer … inner)]
  → Handler + Axum extractors (in parameter order)
  → Response
```

## Per-route stack (`#[routes]` / `impl_routes!`)

For a single HTTP route, nestrs composes Axum middleware roughly as:

1. **Exception filters** — `#[use_filters(F1, F2, …)]`  
   - **First** filter in the list is the **outermost** Tower layer.  
   - On responses carrying an `HttpException`, the **innermost** filter (closest to the handler) runs its `catch` **first**, then the next filter outward.

2. **Controller guard** (only when using `controller_guards(G)` in `impl_routes!`) — a single outer middleware that
   runs **before** route guards on the **incoming** request.

3. **Route guards** — `with (G1, G2, …)` in `impl_routes!`  
   - Evaluated **left-to-right**; the first failure short-circuits.

4. **Interceptors** — `#[use_interceptors(I1, I2, …)]`  
   - **First** interceptor is the **outermost** Tower layer (sees the request first, wraps `next`).

5. **Handler + extractors** — Axum runs extractors in function-parameter order.  
   **`#[use_pipes(ValidationPipe)]`** switches `#[param::body]` / `query` / `param` wiring to `ValidatedBody` / `ValidatedQuery` / `ValidatedPath` (validation at extraction time).

> **Note:** NestJS ordering differs in details; treat this page as the **nestrs contract**, not a line-for-line Nest clone.

### NestJS comparison (mental model only)

In Nest, **guards → interceptors → pipes → route handler** is the common teaching order. In nestrs HTTP routes, **exception filters** and **route guard** ordering are spelled out above; **interceptors** wrap after guards on the inward pass. When porting Nest code, **do not** assume the same relative order for every cross-cutting type—write a small integration test or refer to **`cross_cutting_ordering_contract.rs`** in the repository if you rely on a specific sequence.

## Global `NestApplication::build_router` stack

Global middleware is assembled in `NestApplication::build_router` (`nestrs/src/lib.rs`). Axum’s `Router::layer` means **each new `.layer(...)` wraps outside the previous stack**, so **later calls in `build_router` are generally “more outer” on the incoming request**.

The exact sequence includes (among others): optional global exception filter, CORS, security headers, rate limits, timeouts, request id, compression, CSRF (when enabled), cookie/session layers, then user `use_global_layer` callbacks.

**Do not rely on undocumented ordering between unrelated third-party layers** you add via `use_global_layer`; prefer explicit integration tests for your app.

## Related

- [NestJS migration guide](nestjs-migration.md) — parity table and pitfalls
- [Secure defaults checklist](secure-defaults.md)
