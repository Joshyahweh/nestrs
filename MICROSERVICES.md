# Microservices and Events

This document tracks nestrs parity direction for message-based services and event-driven workflows.

## Available primitives

- `nestrs-microservices::Transport`
  - `send_json(pattern, payload)` for request/reply flows.
  - `emit_json(pattern, payload)` for fire-and-forget events.
- `nestrs-microservices::ClientProxy`
  - Typed wrapper over `Transport` with `send<TReq, TRes>` and `emit<TReq>`.
- `nestrs-microservices::EventBus`
  - In-process async event bus with subscribe + emit semantics.

## Pattern decorators (API surface)

The macro surface now includes:

- `#[message_pattern("...")]`
- `#[event_pattern("...")]`
- `#[use_micro_interceptors(Type, ...)]` / `#[use_micro_guards(Type, ...)]` / `#[use_micro_pipes(Type, ...)]` on micro handlers (order: interceptors → guards → pipes)
- `#[on_event("...")]`
- `#[event_routes]` (impl-block macro that wires `#[on_event]` handlers)

`#[message_pattern]` / `#[event_pattern]` are activated by `#[micro_routes]`.

`#[on_event]` handlers are activated by `#[event_routes]` and are auto-subscribed at runtime when
the app boots (via `EventBus`).

## ClientsModule

`ClientsModule::register(&[ClientConfig { ... }])` returns a `DynamicModule` that exports:

- `ClientsService` (lookup by name: `clients.expect("USER_SERVICE")`)
- `EventBus`
- `ClientProxy` only when exactly one client is registered (default client)

## Extra transports (feature flags)

The `nestrs-microservices` crate supports additional adapters behind feature flags:

- `nats` (NATS request/reply + publish/subscribe)
- `redis` (Redis pub/sub request/reply + fire-and-forget)
- `grpc` (tonic-based gRPC send/emit service)
- `kafka` / `mqtt` (see crate README for options)
- `rabbitmq` (AMQP work queue + per-request reply queues; umbrella feature `microservices-rabbitmq`)

## Custom transporters

Implement `nestrs_microservices::Transport` for your SDK. For servers, deserialize `nestrs_microservices::wire::WireRequest` and call `wire::dispatch_send` / `wire::dispatch_emit` — see the `custom` module in that crate.

## Exception filters vs errors

There is no separate microservice exception-filter pipeline like Nest’s docs. Handlers return `TransportError` or (via generated code) map `HttpException` into `TransportError` with JSON `details`. Use `#[use_micro_guards]` / `#[use_micro_pipes]` for early rejection or payload shaping.

## HTTP vs microservice cross-cutting (parity cheat sheet)

| Concern | HTTP (`nestrs`) | Microservices (`#[micro_routes]`) |
|--------|-----------------|-----------------------------------|
| Before handler | `CanActivate` guards, `PipeTransform`, `Interceptor` | `MicroCanActivate`, `MicroPipeTransform`, `MicroIncomingInterceptor` |
| Order (generated) | guards → pipes → handler (interceptors vary by layer) | **interceptors → guards → pipes → handler** |
| Global exception filter | `use_global_exception_filter` + `HttpException` in response extensions | **No** — return `Result<_, TransportError>`; guards/pipes return `TransportError` |
| Metadata / OpenAPI | `MetadataRegistry`, `#[roles]`, OpenAPI attrs | Pattern string + JSON payload only (no shared HTTP metadata registry on the wire) |

Full narrative: [GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md) in the mdBook (see `docs/src/graphql-ws-micro-dx.md` in the repo).

## JSON wire contract & tests

Transports that carry [`nestrs_microservices::wire::WireRequest`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/wire/struct.WireRequest.html) / [`WireResponse`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/wire/struct.WireResponse.html) share one JSON shape (Redis, Kafka, MQTT, RabbitMQ, custom, and the JSON inside gRPC). **Golden tests:** `nestrs-microservices/tests/wire_conformance.rs` + `tests/fixtures/*.json`. Revision: `nestrs_microservices::WIRE_FORMAT_DOC_REVISION`.

## gRPC (umbrella feature `microservices-grpc`)

Use [`NestFactory::create_microservice_grpc`](https://docs.rs/nestrs/latest/nestrs/struct.NestFactory.html#method.create_microservice_grpc) with [`GrpcMicroserviceOptions::bind(addr)`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.GrpcMicroserviceOptions.html). Clients: [`GrpcTransportOptions::new("http://…")`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.GrpcTransportOptions.html) and chain `.with_request_timeout(Duration::from_secs(30))` when defaults are too tight.

## Integration events guidance

- Use `emit` for integration events (`order.created`, `user.updated`, etc.).
- Use `send` for request/reply patterns where a response contract is required.
- Keep event payloads versioned (e.g. include `event_version`).

## CQRS and outbox guidance

- CQRS is optional and should be layered on top of the transport/event APIs.
- For critical integration events, use the outbox pattern:
  - Write business state + outbox row in one DB transaction.
  - Publish outbox rows asynchronously with retries.
  - Use idempotency keys on consumers.

## Reliability notes

- Assume at-least-once delivery; handlers must be idempotent.
- Include correlation/request IDs in payload metadata.
- Add dead-letter/retry strategy per transport adapter.
