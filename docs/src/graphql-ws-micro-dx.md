# GraphQL, WebSockets & microservices DX

This chapter ties together **partial** areas from the [roadmap](roadmap-parity.md): GraphQL ecosystem boundaries, **WebSocket “exception filter”** semantics, **microservice** guard/pipe/filter parity, the **JSON wire** contract, and **gRPC** usage.

If you are new to nestrs but know **NestJS**, start with the [NestJS → nestrs migration guide](nestjs-migration.md) for HTTP module/decorator mapping before diving into GraphQL/WebSocket differences here.

## GraphQL

Async-GraphQL integration lives in **`nestrs-graphql`**. A lightweight **federation gateway** now ships in-tree behind the **`graphql-federation-gateway`** feature (details below); plugins and codegen remain external — use **async-graphql** + Apollo Router / GraphOS / codegen crates. See the roadmap **GraphQL** row and `nestrs-graphql` crate docs.

| Task in Nest | Practical nestrs approach |
|----------------|---------------------------|
| `@nestjs/graphql` code-first schema | Define async-graphql `Object`/`InputObject` types; pass a built `Schema` to `NestFactory::...enable_graphql(...)`. |
| Federation / subgraph stitching | Lightweight gateway in-tree (**`graphql-federation-gateway`** feature) for SDL stitching + entity dispatch; query planning / type merging stays with Apollo Router or GraphOS. |
| DataLoader / N+1 | Use async-graphql `dataloader` or application-level batching in resolvers—same ecosystem as standalone GraphQL servers. |

### Federation gateway

The **`graphql-federation-gateway`** feature (implies **`graphql-authz`**) turns on a lightweight Apollo Federation gateway: subgraph SDLs stitched behind one Axum endpoint, with `_service { sdl }` introspection and cross-subgraph entity resolution. It is **not** a query planner and does **not** auto-merge type fields — run Apollo Router / GraphOS in front when you need planning.

- **Subgraph SDL export:** [`export_schema_sdl_with_options`](https://docs.rs/nestrs-graphql/latest/nestrs_graphql/fn.export_schema_sdl_with_options.html) with `SDLExportOptions::default().federation()` — the same federation-v2 flags async-graphql uses per subgraph (`@link`, `_Any` / `_service` plumbing). Hand-rolled federation v2 SDL works too.
- **Wiring:** one [`SubgraphSpec`](https://docs.rs/nestrs-graphql/latest/nestrs_graphql/federation/struct.SubgraphSpec.html) (`name` + `sdl` + `entity_resolver`) per subgraph inside [`FederationConfig`](https://docs.rs/nestrs-graphql/latest/nestrs_graphql/federation/struct.FederationConfig.html), then [`federation_router(cfg, "/graphql")`](https://docs.rs/nestrs-graphql/latest/nestrs_graphql/federation/fn.federation_router.html) → `Result<Router, FederationError>`; merge into a `NestApplication` via `use_global_layer(|router| router.merge(gateway))`. All SDLs are validated at construction time (`FederationError::Parse`); duplicate subgraph names fail (`FederationError::Merge`); an empty subgraph list fails (`FederationError::NoSubgraphs`). Dispatch is keyed by `SubgraphSpec.name` matching the representation's `__typename`.
- **Batched `EntityResolver`:** the trait takes `&[&serde_json::Value]` (representations grouped by `__typename`) and returns `Vec<Option<serde_json::Value>>` — **one resolver call per typename per request**, so DataLoader-style batching lives inside the resolver (one DB round-trip per group, not per representation). One entry per input, in input order; `Ok(None)` maps to JSON `null`; unknown `__typename` also resolves to `null`. Any `Fn(&Context, &[&Value]) -> Result<Vec<Option<Value>>>` closure implements the trait via `Arc::new(closure)` — no manual `impl` needed.
- **`entities` field-name quirk:** async-graphql 7 registers the federation entity field as **`entities`** (no underscore) on the gateway schema even though the federation-v2 SDL names it `_entities` per the Apollo spec. Clients calling the gateway directly query `entities(representations: [...])`; an Apollo Router in front of the gateway resolves via the spec-correct name from the SDL.
- **Authz:** [`federation_router_with_hook`](https://docs.rs/nestrs-graphql/latest/nestrs_graphql/federation/fn.federation_router_with_hook.html) wraps `schema.execute_batch` in a **`GqlHandlerHook`** (pass `Arc<GqlDataContext>` for row-level authz) — entity resolvers run inside the hook's scope, so per-request abilities, transactions, and dataloaders are visible to them.

Tests live in `nestrs/tests/graphql_federation_gateway.rs` (two-subgraph round-trip, typename routing, unknown-type `null`, batched dispatch, merged SDL, `@link` directive).

**HTTP surface:** With the **`graphql`** feature, [`NestFactory::enable_graphql`](https://docs.rs/nestrs/latest/nestrs/struct.NestFactory.html#method.enable_graphql) mounts **GET/POST `/graphql`** on the same Axum router as REST controllers (global prefix and versioning apply). You still define resolvers and schema using **async-graphql** APIs; nestrs wires transport and DI around them.

## WebSockets: errors vs HTTP exception filters

HTTP responses can flow through [`NestApplication::use_global_exception_filter`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_global_exception_filter) when handlers attach [`HttpException`](https://docs.rs/nestrs/latest/nestrs/struct.HttpException.html) to the response.

**WebSocket JSON frames do not go through that pipeline.** The [`nestrs-ws`](https://docs.rs/nestrs-ws) crate and `#[ws_routes]` generated code send failures on the event name **`nestrs_ws::WS_ERROR_EVENT`** (`"error"`) with JSON bodies documented in **`nestrs-ws`’s crate-level docs** (guards, pipes, unknown events, bad DTO deserialize, wire parse errors).

**Practical mapping from Nest:** treat per-connection error frames as your gateway’s “exception filter” surface — use shared **`WsCanActivate`** / **`WsPipeTransform`** types or a thin wrapper around [`WsGateway::on_message`](https://docs.rs/nestrs-ws/latest/nestrs_ws/trait.WsGateway.html) if you need one place to normalize payloads.

**DI resolution:** gateways mounted by **`#[ws_gateway]`** dispatch through the registry-aware impl generated by **`#[ws_routes]`**: guard/pipe/interceptor instances are built per message via `WsCanActivate::resolve(&ProviderRegistry)` / `WsPipeTransform::resolve` / `WsIncomingInterceptor::resolve`, so DI-backed guards actually receive their dependencies (mirror of HTTP `CanActivate::resolve`). Hand-written gateways mounted via the plain `ws_route` family keep `Default` construction.

**Origin checks (CSWSH):** WebSocket upgrades are not covered by CORS — a malicious page can open a WebSocket from the victim's browser and call protected handlers as the victim (**Cross-Site WebSocket Hijacking**). `ws_route` and `ws_route_with_guards` do **not** validate the `Origin` header, and neither does the **`#[ws_gateway]`** macro mount (it takes only `path`); those entry points are for gateways behind a trusted reverse proxy that enforces an allowlist. Browser-facing gateways should hand-mount with **`ws_route_with_security`** / **`ws_route_with_guards_and_security`** and a **[`WsSecurityConfig`](https://docs.rs/nestrs-ws/latest/nestrs_ws/struct.WsSecurityConfig.html)**:

- `WsSecurityConfig::allow_origins(["https://app.example.com"])` — entries compared against the `Origin` header **verbatim** (full origin including scheme; no wildcards).
- A `null` Origin (sandboxed iframes, `file://`) is **always rejected** when any allowlist entry is configured; upgrades with **no** Origin header are accepted by default — add `.require_origin(true)` to reject them (browser-only gateways).
- `WsSecurityConfig::allow_off()` accepts every origin (the legacy `ws_route` behavior).
- Rejected origins get a hard **HTTP 403** before the upgrade; an upgrade-**guard** rejection instead accepts the upgrade and immediately closes with **1008 Policy Violation**. When both are configured (`ws_route_with_guards_and_security`), security runs first.

## Microservices: guards, pipes, interceptors, filters

On **`#[micro_routes]`** handlers:

- **`#[use_micro_interceptors(...)]`**, **`#[use_micro_guards(...)]`**, **`#[use_micro_pipes(...)]`**
- Run in order: **interceptors → guards → pipes → handler**
- **DI resolution:** every transport dispatches through the registry-aware
  impl generated by `#[micro_routes]`, so guard/pipe/interceptor instances
  are built per message via `MicroCanActivate::resolve(&ProviderRegistry)` /
  `MicroPipeTransform::resolve` / `MicroIncomingInterceptor::resolve`.
  Override `resolve` to pull injected dependencies (mirror of HTTP
  `CanActivate::resolve`); stateless types that only implement `Default`
  keep working unchanged. Dispatching a bare `Arc<T>` you resolved yourself
  (outside `#[module(microservices = [...])]`) keeps `Default` construction.

There is **no** microservice analogue of Nest’s **exception filter** stack: failures are **`TransportError`** (and `HttpException` is mapped into it in generated code). See the root [`MICROSERVICES.md`](../../MICROSERVICES.md) (also included in [Microservices](microservices.md)) for the HTTP vs micro parity table and wire-format notes.

```text
Micro route (conceptual):

  request → micro interceptors (outer … inner)
         → micro guards (left … right)
         → micro pipes
         → handler
```

Compare with [HTTP pipeline order](http-pipeline-order.md): HTTP runs **filters → (controller guard) → route guards → interceptors → handler**—do not assume the two stacks reorder the same cross-cutting types.

## JSON `wire` format (conformance)

All Redis/Kafka/MQTT/RabbitMQ/custom adapters that use [`nestrs_microservices::wire`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/wire/index.html) share the same **`WireRequest`** / **`WireResponse`** JSON. **gRPC** carries the same JSON inside protobuf bytes.

- **Stability marker:** [`WIRE_FORMAT_DOC_REVISION`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/constant.WIRE_FORMAT_DOC_REVISION.html)
- **Golden tests:** `nestrs-microservices/tests/wire_conformance.rs` and `tests/fixtures/*.json` in that crate — run `cargo test -p nestrs-microservices --test wire_conformance` when changing serde on those types.

## gRPC ergonomics

Enable **`microservices`** + **`microservices-grpc`** on **`nestrs`**.

- **Server:** [`NestFactory::create_microservice_grpc`](https://docs.rs/nestrs/latest/nestrs/struct.NestFactory.html#method.create_microservice_grpc) with [`GrpcMicroserviceOptions::bind`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.GrpcMicroserviceOptions.html).
- **Client transport:** [`GrpcTransportOptions::new`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.GrpcTransportOptions.html) and [`.with_request_timeout`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.GrpcTransportOptions.html#method.with_request_timeout) for long-running RPCs.

## See also

- [Backend stack recipes](backend-recipes.md) — procedural REST / GraphQL / gRPC × Postgres + Prisma or MongoDB  
- [API cookbook](appendix-api-cookbook.md) — `enable_graphql` pointer (async-graphql `Schema` required)  
- [Microservices](microservices.md) (includes `MICROSERVICES.md`)
- [Security](security.md)
- `nestrs-ws/README.md`, `nestrs-microservices/README.md`
