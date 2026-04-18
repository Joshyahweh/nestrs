# Observability

This page describes a **single golden path** for a typical [`NestFactory`](https://docs.rs/nestrs/latest/nestrs/struct.NestFactory.html) HTTP app: install `tracing`, add request logs + **spans**, expose **Prometheus** metrics, and optionally export traces to an OTLP collector.

**More snippets:** Additional copy-paste blocks for **`configure_tracing`**, **`use_request_id`**, **`use_request_context`**, **`use_execution_context`**, **`enable_health_check`**, **`enable_readiness_check`**, and **`enable_metrics`** live in the [API cookbook](appendix-api-cookbook.md).

## 1. Install the global `tracing` subscriber

Call [`NestApplication::configure_tracing`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.configure_tracing) **once**, before [`listen`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.listen), so all log lines and spans share one pipeline.

Filtering uses **`NESTRS_LOG`** if set, else **`RUST_LOG`**, else the default directive from [`TracingConfig`](https://docs.rs/nestrs/latest/nestrs/struct.TracingConfig.html) (default `"info"`).

```rust
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    let tracing = TracingConfig::builder()
        .format(TracingFormat::Json) // Pretty for local dev
        .default_directive("info,nestrs=debug");

    NestFactory::create::<AppModule>()
        .configure_tracing(tracing)
        // … chain more builders below …
        .listen(3000)
        .await;
}
```

## 2. Request tracing middleware (logs + spans)

[`NestApplication::use_request_tracing`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_request_tracing) records a completion line with `method`, `path`, `status`, `duration_ms`, and `request_id` when the `x-request-id` header is present.

It also creates a **`tracing` span** for each request (skipped for paths in [`RequestTracingOptions`](https://docs.rs/nestrs/latest/nestrs/struct.RequestTracingOptions.html), e.g. `/metrics`):

- **Span name:** `http.server.request` (aligned with OpenTelemetry HTTP server semantics).
- **Fields:** `http.request.method`, `http.route` (see below).

`http.route` is set to the **request path** as seen by this middleware (the literal URI path). Axum’s **route template** (e.g. `/api/users/:id`) is not available at this layer, so traces show the concrete path; for OTLP dashboards, treat it as the closest stable route identifier unless you add a custom layer that sets a template field.

Example (typical for metrics scrape + health):

```rust
NestFactory::create::<AppModule>()
    .configure_tracing(TracingConfig::builder())
    .use_request_id()
    .use_request_tracing(RequestTracingOptions::builder().skip_paths(["/metrics", "/health"]))
    .enable_metrics("/metrics")
    .enable_health_check("/health")
    .listen(3000)
    .await;
```

## 3. Prometheus metrics

[`NestApplication::enable_metrics`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.enable_metrics) registers a Prometheus scrape handler (histogram `http_request_duration_seconds`, counters, in-flight gauge, etc.). Keep `/metrics` in [`RequestTracingOptions::skip_paths`](https://docs.rs/nestrs/latest/nestrs/struct.RequestTracingOptions.html) so scrapes do not flood request logs.

## 4. Optional: OpenTelemetry (OTLP)

Enable the **`otel`** feature and use [`configure_tracing_opentelemetry`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.configure_tracing_opentelemetry) instead of (or after the same pattern as) `configure_tracing`. This keeps [`TracingConfig`](https://docs.rs/nestrs/latest/nestrs/struct.TracingConfig.html) formatting and adds a `tracing-opentelemetry` layer that exports spans (including `http.server.request`) to an OTLP endpoint.

**`Cargo.toml`:**

```toml
[dependencies]
nestrs = { version = "0.3.8", features = ["otel"] }
```

**`main`:**

```rust
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    let tracing = TracingConfig::builder().format(TracingFormat::Json);
    let otel = OpenTelemetryConfig::new("my-service")
        .endpoint("http://localhost:4317")
        .sample_ratio(1.0);

    NestFactory::create::<AppModule>()
        .configure_tracing_opentelemetry(tracing, otel)
        .use_request_id()
        .use_request_tracing(RequestTracingOptions::builder().skip_paths(["/metrics"]))
        .enable_metrics("/metrics")
        .listen(3000)
        .await;
}
```

### Environment variables

- **`OTEL_EXPORTER_OTLP_ENDPOINT`**: used when `OpenTelemetryConfig::endpoint(...)` is not set (default collector address falls back to `http://localhost:4317`).

See also: [Production runbook](production.md) for deployment-oriented notes.

## Troubleshooting

| Symptom | Things to check |
|---------|------------------|
| No log lines at all | `configure_tracing` must run **before** `listen`; verify `NESTRS_LOG` / `RUST_LOG` and that nothing else installs a conflicting subscriber. |
| `/metrics` floods access logs | Add `/metrics` to `RequestTracingOptions::skip_paths` (shown above). |
| Spans missing in Jaeger/Tempo | Confirm `otel` feature, endpoint URL, and sampling ratio; verify the collector receives traffic on the expected gRPC/HTTP port. |
| High cardinality in `http.route` | Expected: path is literal at this layer; add a custom layer or business metric if you need template-level labels. |

## Environment variables (quick reference)

| Variable | Role |
|----------|------|
| `NESTRS_LOG` | Preferred filter directive for nestrs tracing when set (overrides default in `TracingConfig`). |
| `RUST_LOG` | Fallback if `NESTRS_LOG` is unset (standard `tracing-subscriber` semantics). |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP collector address when not set explicitly in `OpenTelemetryConfig`. |

## Local development vs production

- **Local**: `TracingFormat::Pretty` or human-readable JSON to stdout; relaxed log levels (`debug` for `nestrs`).  
- **Production**: Structured JSON (`TracingFormat::Json`), consistent `service.name` in OTel resource, scrape `/metrics` from Prometheus, and aggregate logs to your platform (Loki, CloudWatch, etc.).  
