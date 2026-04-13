# GraphQL, WebSockets & microservices DX

This chapter ties together **partial** areas from the [roadmap](roadmap-parity.md): GraphQL ecosystem boundaries, **WebSocket “exception filter”** semantics, **microservice** guard/pipe/filter parity, the **JSON wire** contract, and **gRPC** usage.

## GraphQL

Async-GraphQL integration lives in **`nestrs-graphql`**. Nest parity for federation, plugins, and codegen is explicitly **out of core** — use **async-graphql** + Apollo Router / GraphOS / codegen crates. See the roadmap **GraphQL** row and `nestrs-graphql` crate docs.

## WebSockets: errors vs HTTP exception filters

HTTP responses can flow through [`NestApplication::use_global_exception_filter`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_global_exception_filter) when handlers attach [`HttpException`](https://docs.rs/nestrs/latest/nestrs/struct.HttpException.html) to the response.

**WebSocket JSON frames do not go through that pipeline.** The [`nestrs-ws`](https://docs.rs/nestrs-ws) crate and `#[ws_routes]` generated code send failures on the event name **`nestrs_ws::WS_ERROR_EVENT`** (`"error"`) with JSON bodies documented in **`nestrs-ws`’s crate-level docs** (guards, pipes, unknown events, bad DTO deserialize, wire parse errors).

**Practical mapping from Nest:** treat per-connection error frames as your gateway’s “exception filter” surface — use shared **`WsCanActivate`** / **`WsPipeTransform`** types or a thin wrapper around [`WsGateway::on_message`](https://docs.rs/nestrs-ws/latest/nestrs_ws/trait.WsGateway.html) if you need one place to normalize payloads.

## Microservices: guards, pipes, interceptors, filters

On **`#[micro_routes]`** handlers:

- **`#[use_micro_interceptors(...)]`**, **`#[use_micro_guards(...)]`**, **`#[use_micro_pipes(...)]`**
- Run in order: **interceptors → guards → pipes → handler**

There is **no** microservice analogue of Nest’s **exception filter** stack: failures are **`TransportError`** (and `HttpException` is mapped into it in generated code). See the root [`MICROSERVICES.md`](../../MICROSERVICES.md) (also included in [Microservices](microservices.md)) for the HTTP vs micro parity table and wire-format notes.

## JSON `wire` format (conformance)

All Redis/Kafka/MQTT/RabbitMQ/custom adapters that use [`nestrs_microservices::wire`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/wire/index.html) share the same **`WireRequest`** / **`WireResponse`** JSON. **gRPC** carries the same JSON inside protobuf bytes.

- **Stability marker:** [`WIRE_FORMAT_DOC_REVISION`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/constant.WIRE_FORMAT_DOC_REVISION.html)
- **Golden tests:** `nestrs-microservices/tests/wire_conformance.rs` and `tests/fixtures/*.json` in that crate — run `cargo test -p nestrs-microservices --test wire_conformance` when changing serde on those types.

## gRPC ergonomics

Enable **`microservices`** + **`microservices-grpc`** on **`nestrs`**.

- **Server:** [`NestFactory::create_microservice_grpc`](https://docs.rs/nestrs/latest/nestrs/struct.NestFactory.html#method.create_microservice_grpc) with [`GrpcMicroserviceOptions::bind`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.GrpcMicroserviceOptions.html).
- **Client transport:** [`GrpcTransportOptions::new`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.GrpcTransportOptions.html) and [`.with_request_timeout`](https://docs.rs/nestrs-microservices/latest/nestrs_microservices/struct.GrpcTransportOptions.html#method.with_request_timeout) for long-running RPCs.

## See also

- [Microservices](microservices.md) (includes `MICROSERVICES.md`)
- [Security](security.md)
- `nestrs-ws/README.md`, `nestrs-microservices/README.md`
