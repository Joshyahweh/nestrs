# Roadmap parity checklist

This page tracks Nest-style capabilities and where they live in `nestrs`.

| Area | Status | Notes |
|------|--------|--------|
| URI / header / `Accept` versioning | **Done** | `enable_uri_versioning`, `enable_header_versioning`, `enable_media_type_versioning`, `NestApiVersion` |
| Host / subdomain routes | **Done** | `#[controller(host = "...")]` + host middleware on controller routers |
| Redis rate limiting | **Done** | `RateLimitOptions::redis(...)` with **`cache-redis`** |
| Kafka / MQTT transports | **Done** (features) | `rskafka` / `rumqttc`; TLS + SASL (Kafka), username/password + TLS (MQTT); optional **`microservices-metrics`** (`nestrs` feature) |
| Broker readiness | **Partial** | `RedisBrokerHealth` / `NatsBrokerHealth`; Kafka `kafka_cluster_reachable` / `kafka_cluster_reachable_with`; topic retention via cluster ops (not client `create_topic` attrs) |
| CQRS sagas | **Traits** | `Saga`, `SagaDefinition` in `nestrs-cqrs` |
| Event bus crate | **Done** | `nestrs-events` (`EventBus`) |
| Prisma / SQL connectivity | **Done** | **`sqlx`**: `ping()`, `query_scalar()`, `query_all_as`, `execute`; models in `schema.prisma` + Rust `FromRow` or generated client (`cargo prisma generate`) — see `nestrs-prisma/README.md` |
| GraphQL production limits | **Done** | `nestrs-graphql::limits`: `with_default_limits` (`SchemaBuilder::limit_depth` / `limit_complexity`), `Analyzer` |
| DTO validators (Nest-like) | **Partial** | `#[dto]` maps `Min`/`Max`/`IsUrl`/`ValidateNested`/… to `validator` |
| RFC 9457 problems | **Done** | `ProblemDetails` |
| OTel log correlation | **Doc** | Spans + `tracing`; OTLP logs via collector until stable exporters |

See [`nestrs-plan-2.md`](../../nestrs-plan-2.md) in the repo root for the full phased roadmap narrative.
