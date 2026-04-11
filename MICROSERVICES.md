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
