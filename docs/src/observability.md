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

## 3. Health, readiness, and probe decorators

**Liveness** (`enable_health_check("/health")`) is a constant `200 {"status":"ok"}` — use it for "should this process be restarted".

**Readiness** (`enable_readiness_check("/ready", [indicators])`) runs your **`HealthIndicator`** implementations on each scrape; any `Down` ⇒ `503` with a Terminus-style JSON body (`status`, `info`, `error`, `details`) — use it for "should this pod receive traffic".

**Probe decorators** mirror a *real handler* as a fixed probe endpoint:

| Decorator | Mirrored endpoint | Semantics |
|-----------|-------------------|-----------|
| **`#[liveness]`** | `GET /__nestrs/health/live` | 2xx from the handler ⇒ up, otherwise down |
| **`#[readiness]`** | `GET /__nestrs/health/ready` | 2xx ⇒ up, otherwise down |
| **`#[startup]`** | `GET /__nestrs/health/startup` | Evaluated **once per process** and cached |

```rust
#[routes(state = AppState)]
impl HealthController {
    #[get("/health/deep")]
    #[readiness]
    pub async fn deep(State(s): State<AppState>) -> impl IntoResponse {
        match s.check_deps().await {
            Ok(_) => (StatusCode::OK, Json(json!({"deps": "ok"}))),
            Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"deps": "down"}))),
        }
    }
}
```

- Probe endpoints mount at the **server root** (not under `set_global_prefix` / URI versioning) so orchestrators probe without prefixes.
- Results are cached for **5 seconds** per endpoint (probe-storm self-DoS guard); a **panicking** handler reports down with a generic message (raw errors go to `tracing` only).
- Route collisions: if your own routes already occupy `/__nestrs/health/live|ready`, **your routes win** — the framework skips its mirrored endpoint.
- Kubernetes wiring: `livenessProbe` → `/__nestrs/health/live` (or your `enable_health_check` path), `readinessProbe` → `/__nestrs/health/ready`, `startupProbe` → `/__nestrs/health/startup`.

Use decorators when liveness should depend on something the app actually does; use the builder calls when a constant 200 plus indicator checks is enough.

## 4. Prometheus metrics

[`NestApplication::enable_metrics`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.enable_metrics) registers a Prometheus scrape handler (histogram `http_request_duration_seconds`, counters, in-flight gauge, etc.). Keep `/metrics` in [`RequestTracingOptions::skip_paths`](https://docs.rs/nestrs/latest/nestrs/struct.RequestTracingOptions.html) so scrapes do not flood request logs.

## 5. Optional: OpenTelemetry (OTLP)

Enable the **`otel`** feature and use [`configure_tracing_opentelemetry`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.configure_tracing_opentelemetry) instead of (or after the same pattern as) `configure_tracing`. This keeps [`TracingConfig`](https://docs.rs/nestrs/latest/nestrs/struct.TracingConfig.html) formatting and adds a `tracing-opentelemetry` layer that exports spans (including `http.server.request`) to an OTLP endpoint.

**Spans export by default; metrics and logs are per-signal opt-ins** on [`OpenTelemetryConfig`](https://docs.rs/nestrs/latest/nestrs/otel/struct.OpenTelemetryConfig.html): `.metrics()` pushes every `metrics`-facade instrument to the collector, `.logs()` bridges every `tracing::*!` event into the OTLP log pipeline (correlated with the active trace/span). Local stdout output is unchanged — OTLP is an additional destination.

**`Cargo.toml`:**

```toml
[dependencies]
nestrs = { version = "1.5.0", features = ["otel"] }
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
        .sample_ratio(1.0)
        .metrics()   // also push metrics over OTLP
        .logs();     // also push logs over OTLP

    NestFactory::create::<AppModule>()
        .configure_tracing_opentelemetry(tracing, otel)
        .use_request_id()
        .use_request_tracing(RequestTracingOptions::builder().skip_paths(["/metrics"]))
        .enable_metrics("/metrics")
        .listen(3000)
        .await;
}
```

### Dual metrics export (Prometheus pull + OTLP push)

There is exactly **one** metrics recording surface — the `metrics` facade — and nestrs owns it with a fan-out recorder. `enable_metrics("/metrics")` registers the Prometheus backend; `OpenTelemetryConfig::metrics()` registers the OTLP backend. Both can run side by side (in either order): the framework's RED metrics **and** your own instruments (`nestrs::metrics::counter!(...)` — the facade is re-exported, no extra dependency) land identical series in Prometheus and in the collector. Facade labels become OTel attributes 1:1, and units translate (`Unit::Seconds` → `s`, `Unit::Count` → `1`).

**Runtime notes:** the OTLP pipelines are lazy (a missing collector never fails startup) but must be **constructed from async context** — call `configure_tracing_opentelemetry` from `#[tokio::main]`, as above. Metric pushes are periodic (every 60 s by default; `OTEL_METRIC_EXPORT_INTERVAL` in ms). The `listen*` methods flush and stop all OTLP pipelines on graceful shutdown.

### Environment variables

- **`OTEL_EXPORTER_OTLP_ENDPOINT`**: used when `OpenTelemetryConfig::endpoint(...)` is not set (default collector address falls back to `http://localhost:4317`).
- **`OTEL_METRIC_EXPORT_INTERVAL`**: OTLP metric push cadence in milliseconds (default 60000).

See also: [Production runbook](production.md) for deployment-oriented notes.

## Troubleshooting

| Symptom | Things to check |
|---------|------------------|
| No log lines at all | `configure_tracing` must run **before** `listen`; verify `NESTRS_LOG` / `RUST_LOG` and that nothing else installs a conflicting subscriber. |
| `/metrics` floods access logs | Add `/metrics` to `RequestTracingOptions::skip_paths` (shown above). |
| Spans missing in Jaeger/Tempo | Confirm `otel` feature, endpoint URL, and sampling ratio; verify the collector receives traffic on the expected gRPC/HTTP port. |
| OTel metrics/logs missing | Confirm `.metrics()` / `.logs()` on the config (spans export without them). Metric pushes are periodic — wait one `OTEL_METRIC_EXPORT_INTERVAL` before concluding failure. |
| `there is no reactor running` panic | OTLP pipelines need a current Tokio reactor at construction: call `configure_tracing_opentelemetry` from inside `#[tokio::main]`, not from plain `fn main`. |
| High cardinality in `http.route` | Expected: path is literal at this layer; add a custom layer or business metric if you need template-level labels. |

## Environment variables (quick reference)

| Variable | Role |
|----------|------|
| `NESTRS_LOG` | Preferred filter directive for nestrs tracing when set (overrides default in `TracingConfig`). |
| `RUST_LOG` | Fallback if `NESTRS_LOG` is unset (standard `tracing-subscriber` semantics). |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP collector address when not set explicitly in `OpenTelemetryConfig`. |
| `OTEL_METRIC_EXPORT_INTERVAL` | OTLP metric push cadence in ms (default 60000). | |

## Local development vs production

- **Local**: `TracingFormat::Pretty` or human-readable JSON to stdout; relaxed log levels (`debug` for `nestrs`).  
- **Production**: Structured JSON (`TracingFormat::Json`), consistent `service.name` in OTel resource, scrape `/metrics` from Prometheus, and aggregate logs to your platform (Loki, CloudWatch, etc.).  
