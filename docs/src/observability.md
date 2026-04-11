# Observability

## OpenTelemetry (OTLP)

`nestrs` exposes an optional OpenTelemetry integration surface via the `otel` feature. It wires a `tracing` subscriber that:

- keeps the existing `TracingConfig` formatting (`Pretty` / `Json`)
- exports spans to an OTLP collector via `tracing-opentelemetry`

### Enable the feature

In your app `Cargo.toml`:

```toml
[dependencies]
nestrs = { path = "../nestrs", features = ["otel"] }
```

### Configure tracing + OpenTelemetry

```rust
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    let tracing = TracingConfig::builder().format(TracingFormat::Json);
    let otel = OpenTelemetryConfig::new("my-service")
        // Optional: defaults to OTEL_EXPORTER_OTLP_ENDPOINT or http://localhost:4317
        .endpoint("http://localhost:4317")
        .sample_ratio(1.0);

    NestFactory::create::<AppModule>()
        .configure_tracing_opentelemetry(tracing, otel)
        .listen(3000)
        .await;
}
```

### Environment variables

- `OTEL_EXPORTER_OTLP_ENDPOINT`: used when `OpenTelemetryConfig::endpoint(...)` is not set.

## Prometheus metrics

`nestrs` already provides a Prometheus scrape endpoint via `NestApplication::enable_metrics(...)`.

