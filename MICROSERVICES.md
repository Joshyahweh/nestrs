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

These are parity placeholders for handler declaration style while transport/runtime adapters evolve.

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
