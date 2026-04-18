# NestJS → nestrs migration guide

This guide maps NestJS concepts to **nestrs** (Rust + Axum + Tower). It is aimed at teams moving HTTP APIs, guards, validation, and modular structure from Node to Rust without losing architectural familiarity.

## Mental model

| NestJS | nestrs |
|--------|--------|
| Module (`@Module`) | `#[module(...)]` on a `struct` |
| Controller (`@Controller`) | `#[controller(prefix = ..., version = ...)]` on a `struct` |
| Provider (`@Injectable`) | `#[injectable]` on a `struct` + registration in `#[module(providers = [...])]` |
| Route handlers (`@Get`, …) | `#[routes]` `impl ControllerType { #[get("path")] async fn ... }` |
| `NestFactory.create` | `NestFactory::create::<AppModule>()` |
| Middleware / interceptors / guards / pipes | Tower layers + `#[use_guards]`, `#[use_interceptors]`, `#[use_pipes]`, `#[use_filters]` (see [HTTP pipeline order](http-pipeline-order.md)) |

## Modules and discovery

- **Nest** composes providers in a module graph; **nestrs** builds a `ProviderRegistry` at compile time via macros and `#[module(...)]`.
- **Dynamic modules** in Nest map to nestrs **dynamic module** patterns described in [Fundamentals](fundamentals.md) (feature-gated re-exports).
- Prefer **constructor injection** in Nest; in nestrs use **`#[injectable]`** types resolved from the registry (`registry.get::<T>()` inside generated wiring).

## Decorators parity (HTTP)

| NestJS | nestrs | Notes |
|--------|--------|-------|
| `@Get()`, `@Post()`, … | `#[get("path")]`, `#[post("path")]`, … | Paths are literal segments; prefix/version come from `#[controller]` |
| `@Body()`, `@Query()`, `@Param()` | `#[param::body]`, `#[param::query]`, `#[param::param]` | With `#[use_pipes(ValidationPipe)]`, body/query/path become `ValidatedBody` / `ValidatedQuery` / `ValidatedPath` |
| `@UseGuards()` | `#[use_guards(GuardTy)]` | Implements `CanActivate` |
| `@UseInterceptors()` | `#[use_interceptors(InterceptorTy)]` | Implements `Interceptor` |
| `@UsePipes()` | `#[use_pipes(ValidationPipe)]` | Validation is opt-in per route or parameter wiring |
| `@UseFilters()` | `#[use_filters(FilterTy)]` | Implements `ExceptionFilter` for `HttpException` responses |
| `@SetMetadata` / custom decorators | `#[set_metadata("k","v")]`, `#[roles("admin")]` | Stored in `MetadataRegistry`; see [Custom decorators](custom-decorators.md) |

## Guards, interceptors, filters (semantics)

- **Guards** answer “may this request proceed?” before the handler runs. On failure they return an HTTP error response (same idea as Nest’s `ForbiddenException`, etc.).
- **Interceptors** wrap the handler: run code before/after `next.run(req)` (logging, headers, timing).
- **Exception filters** rewrite responses that carry an `HttpException` in Axum extensions (global filter via `NestApplication::use_global_exception_filter`, route filters via `#[use_filters]`).

**Ordering** (per route) is part of the public contract; see [HTTP pipeline order](http-pipeline-order.md) and the `impl_routes!` rustdoc on `NestFactory` / `impl_routes`.

## DTOs and validation

- Nest often uses `class-validator` + `class-transformer`. nestrs uses **`#[dto]`** (serde + `validator`) and **`ValidatedBody<T>`** for JSON bodies.
- **Unknown JSON keys** are rejected by default (`#[serde(deny_unknown_fields)]` emitted by `#[dto]`). Opt out with `#[dto(allow_unknown_fields)]` when you intentionally accept forward-compatible clients.

## OpenAPI, GraphQL, WebSockets, microservices

| Nest area | nestrs crate / feature | Starting point |
|-----------|------------------------|----------------|
| Swagger / OpenAPI | `nestrs-openapi`, feature `openapi` | [OpenAPI & HTTP](openapi-http.md) |
| GraphQL | `nestrs-graphql`, feature `graphql` | [GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md) |
| WebSockets gateways | `nestrs-ws`, feature `ws` | Same chapter + `#[ws_gateway]`, `#[ws_routes]` |
| Microservices / messaging | `nestrs-microservices`, features `microservices`, `microservices-*` | [Microservices](microservices.md) |

## Common pitfalls

1. **Global registries** (`RouteRegistry`, `MetadataRegistry`) are process-wide (see `STABILITY.md` at the repository root). Integration tests should use the **`test-hooks`** feature and clear helpers; do not enable `test-hooks` in production binaries.
2. **CSRF** is opt-in: enabling **cookies** or **sessions** without **`use_csrf_protection`** triggers a **runtime warning** at router build. Plan for double-submit or header-only auth for mutations.
3. **CORS**: permissive CORS in production is warned in logs; prefer explicit origins. See [Secure defaults checklist](secure-defaults.md).
4. **Async in constructors**: Rust has no `async constructor`; use **`async` factory providers** or `ModuleRef` patterns from [Fundamentals](fundamentals.md).
5. **`#[dto]` and manual `#[serde(...)]`**: avoid duplicating `deny_unknown_fields` on the same struct unless you know why; the macro applies the default strict shape.

## Configuration (`ConfigModule` → Rust patterns)

Nest’s **`@nestjs/config`** often loads `.env` and exposes a typed `ConfigService`. In nestrs you typically:

1. Parse environment variables in **`main`** (or a small bootstrap module) with `std::env`, [`dotenvy`](https://crates.io/crates/dotenvy), or [`confy`](https://crates.io/crates/confy).  
2. Pass the result into **`ConfigurableModuleBuilder::for_root`** / **`for_root_async`** so injectables receive **[`ModuleOptions<O, M>`](https://docs.rs/nestrs-core/latest/nestrs_core/struct.ModuleOptions.html)**—see the full recipe in [Fundamentals](fundamentals.md).  
3. For **secrets**, prefer your platform’s secret store; **`for_root_async`** exists so you can await a vault client **before** the DI graph constructs singletons.

There is no single `ConfigService` token; use one `#[injectable]` “settings” type per bounded context if that matches your team’s style.

## Middleware (`MiddlewareConsumer` → Axum layers)

Nest’s **`app.use`** / module middleware maps to:

- **Global**: `NestApplication::use_global_layer` and the built-in helpers (`enable_cors`, `use_security_headers`, …) assembled in **`build_router`**.  
- **Route-local**: guards, interceptors, and filters attached with attributes on **`#[routes]`**—ordering is fixed; see [HTTP pipeline order](http-pipeline-order.md).

You do not get Nest’s “middleware for a route prefix only” DSL; model that with **route groups** (separate controllers) or **Axum `Router::nest`** patterns merged into the app.

## Exceptions and HTTP errors

Nest’s **`HttpException`** / **`@HttpCode()`** have nestrs analogues:

- Throw-style errors: build [`HttpException`](https://docs.rs/nestrs/latest/nestrs/struct.HttpException.html) and attach via Axum extensions / filter pipeline (see rustdoc for `NestApplication::use_global_exception_filter`).  
- Status codes: **`#[http_code(201)]`** and similar on handlers where supported by your macro version.

Filters are **not** identical to Nest’s global filter ordering—always verify with integration tests.

## Testing

| Nest (Jest / Supertest) | nestrs |
|-------------------------|--------|
| `TestingModule` | Build `NestFactory::create::<M>().into_router()` and `tower::ServiceExt::oneshot` requests, or use `reqwest` against **`listen`** in tests. |
| Isolated metadata / routes | Enable **`test-hooks`** on **`nestrs`** and use registry clear helpers between tests—see **`STABILITY.md`** and [Fundamentals](fundamentals.md) “Integration tests”. |

`cargo test` runs unit and integration tests in parallel; use **`serial_test`** or explicit synchronization if a test **must** own the only HTTP listener on a port.

## Dependencies and packaging

| Nest / npm | Rust |
|------------|------|
| `package.json` + lockfile | **`Cargo.toml`** + **`Cargo.lock`** (commit lock for binaries). |
| Nest platform packages | Optional **`nestrs-*`** crates (`openapi`, `graphql`, `ws`, …) via **features**. |
| `peerDependencies` | Cargo does not have peers; use **workspace** `[patch]` / version alignment in the root `Cargo.toml`. |

## Minimal side-by-side

**Nest (TypeScript, simplified)**

```typescript
@Controller('cats')
export class CatsController {
  @Get(':id')
  findOne(@Param('id') id: string) { return { id }; }
}
```

**nestrs (Rust, simplified)**

```rust
#[controller(prefix = "/cats", version = "v1")]
struct CatsController;

#[routes(state = AppState)]
impl CatsController {
    #[get("/:id")]
    async fn find_one(#[param::param] p: IdParam) -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({ "id": p.id }))
    }
}
```

(Real apps also need `#[module(...)]`, `AppState`, and `impl_routes!` / `#[routes]` expansion as generated by your project.)

## Where to read next

- [First steps](first-steps.md) — toolchain and first app  
- [Fundamentals](fundamentals.md) — DI, scopes, lifecycle, `forward_ref`, `for_root_async`  
- [Custom decorators](custom-decorators.md) — `#[roles]`, metadata, `ValidatedBody`  
- [Security](security.md) — platform controls + `SECURITY.md` at repo root  
- [HTTP pipeline order](http-pipeline-order.md) — global vs per-route ordering  
- [Roadmap parity](roadmap-parity.md) — full matrix  
- [API cookbook](appendix-api-cookbook.md) — examples for `NestApplication` methods and CLI commands  
