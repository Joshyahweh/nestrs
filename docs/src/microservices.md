# Microservices

The **`nestrs-microservices`** crate (and related features on **`nestrs`**) adds **transports** and a shared **JSON wire** format so handlers written with `#[micro_routes]` can talk to Redis, Kafka, MQTT, RabbitMQ, NATS, or gRPC peers without reinventing envelopes. This page **includes** the repository **`MICROSERVICES.md`** for the authoritative HTTP vs micro parity table and wire notes.

## Choosing a transport (rule of thumb)

| Situation | Consider |
|-----------|----------|
| Already on Kafka / Redpanda | Kafka adapter and consumer groups—see included doc for feature flags. |
| Lowest ops, single region | Redis or NATS, depending on your ops standards. |
| Mobile / IoT style fan-out | MQTT patterns in the crate README. |
| Cross-language RPC with strong contracts | gRPC transport; JSON payload inside protobuf as documented in [GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md). |

**Ordering on micro handlers** differs from HTTP: interceptors run before guards—see [GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md). For HTTP-specific global ordering, use [HTTP pipeline order](http-pipeline-order.md).

**HTTP-side `NestApplication`:** Transport-specific APIs are in **`nestrs-microservices`**; HTTP builder examples (`use_global_layer`, metrics, etc.) are in the [API cookbook](appendix-api-cookbook.md).

---

{{#include ../../MICROSERVICES.md}}

