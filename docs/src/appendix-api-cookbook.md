# API cookbook

This chapter collects **minimal examples** for APIs that appear throughout the book. Patterns match the **`nestrs`** test suite (`nestrs/tests/bootstrap_composition.rs`, `cross_cutting_ordering_contract.rs`, `integration_matrix.rs`, etc.). Replace `AppModule` with your root module.

Code blocks use **`rust,noplayground`** so mdBook does not send **nestrs** snippets to the public Rust Playground (see [Introduction](index.md)).

---

## `NestFactory` and `NestApplication`

### `NestFactory::create` → `listen` / `listen_graceful` / `into_router`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    // HTTP server (default bind 127.0.0.1)
    NestFactory::create::<AppModule>().listen(3000).await;

    // Or: graceful shutdown on Ctrl+C (see rustdoc for behavior)
    // NestFactory::create::<AppModule>().listen_graceful(3000).await;

    // Or: build an Axum `Router` for `tower::Service` tests / custom servers
    // let _router = NestFactory::create::<AppModule>().into_router();
}
```

### `set_global_prefix`

Prefixes REST routes (not `/health`, `/ready`, `/metrics` — see rustdoc).

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<AppModule>()
        .set_global_prefix("api") // e.g. /api/v1/...
        .listen(3000)
        .await;
}
```

### `module_ref`

```rust,noplayground
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[module(providers = [AppState])]
struct AppModule;

fn main() {
    let app = NestFactory::create::<AppModule>();
    let mref = app.module_ref();
    let _state: std::sync::Arc<AppState> = mref.get::<AppState>();
    let _router = app.into_router();
}
```

See [Fundamentals](fundamentals.md) for dynamic resolution patterns.

---

## Tracing and OpenTelemetry

### `configure_tracing`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    let tracing = TracingConfig::builder()
        .format(TracingFormat::Json)
        .default_directive("info,nestrs=debug");

    NestFactory::create::<AppModule>()
        .configure_tracing(tracing)
        .listen(3000)
        .await;
}
```

### `configure_tracing_opentelemetry` (feature **`otel`**)

```rust,noplayground
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
        .listen(3000)
        .await;
}
```

### `use_request_id`, `use_request_tracing`, `enable_metrics`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<AppModule>()
        .configure_tracing(TracingConfig::builder())
        .use_request_id()
        .use_request_tracing(RequestTracingOptions::builder().skip_paths(["/metrics", "/health"]))
        .enable_metrics("/metrics")
        .enable_health_check("/health")
        .listen(3000)
        .await;
}
```

### `use_request_context` and `use_execution_context`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<AppModule>()
        .use_request_id()
        .use_request_context()
        .use_execution_context()
        .listen(3000)
        .await;
}
```

Handlers can then use extractors such as **`HttpExecutionContext`** (see crate rustdoc).

---

## CORS, security headers, limits, backpressure

### `enable_cors`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .enable_cors(
            CorsOptions::builder()
                .allow_origins(["https://app.example.com"])
                .allow_methods(["GET", "POST"])
                .allow_headers(["content-type", "authorization"])
                .allow_credentials(true)
                .build(),
        )
        .into_router();
}
```

### `use_security_headers`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .use_security_headers(SecurityHeaders::helmet_like())
        .into_router();
}
```

### `use_rate_limit`, `use_body_limit`, `use_request_timeout`, `use_concurrency_limit`

```rust,noplayground
use nestrs::prelude::*;
use std::time::Duration;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .use_rate_limit(
            RateLimitOptions::builder()
                .max_requests(100)
                .window_secs(60)
                .build(),
        )
        .use_body_limit(1024 * 1024)
        .use_request_timeout(Duration::from_secs(30))
        .use_concurrency_limit(256)
        .into_router();
}
```

### `use_path_normalization`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .use_path_normalization(PathNormalization::TrimTrailingSlash)
        .into_router();
}
```

### Compression / decompression

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .use_request_decompression()
        .use_compression()
        .into_router();
}
```

### `use_load_shed`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>().use_load_shed().into_router();
}
```

---

## Production errors

### `enable_production_errors` / `enable_production_errors_from_env`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .enable_production_errors_from_env()
        .into_router();
}
```

---

## Cookies, CSRF (features **`cookies`**, **`csrf`**)

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .use_cookies()
        .use_csrf_protection(CsrfProtectionConfig::default())
        .into_router();
}
```

---

## Health, readiness, metrics

### `enable_health_check`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .enable_health_check("/health")
        .into_router();
}
```

### `enable_readiness_check`

```rust,noplayground
use nestrs::prelude::*;

#[derive(Clone, Default)]
struct DbIndicator;

#[async_trait::async_trait]
impl HealthIndicator for DbIndicator {
    fn name(&self) -> &'static str {
        "db"
    }

    async fn check(&self) -> HealthStatus {
        HealthStatus::Up
    }
}

#[module]
struct AppModule;

fn main() {
    let indicators: Vec<std::sync::Arc<dyn HealthIndicator>> =
        vec![std::sync::Arc::new(DbIndicator)];

    let _router = NestFactory::create::<AppModule>()
        .enable_readiness_check("/ready", indicators)
        .into_router();
}
```

---

## URI versioning

### `enable_uri_versioning`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .enable_uri_versioning("v") // paths like /v/v1/...
        .into_router();
}
```

For **`enable_api_versioning`**, **`enable_header_versioning`**, **`enable_media_type_versioning`**, see rustdoc on [`NestApplication`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html) and integration tests.

---

## OpenAPI and GraphQL

### `enable_openapi` / `enable_openapi_with_options`

Requires **`features = ["openapi"]`** on the **`nestrs`** dependency.

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<AppModule>()
        .enable_openapi() // GET /openapi.json, GET /docs
        .listen(3000)
        .await;
}
```

See [OpenAPI & HTTP](openapi-http.md) for `OpenApiOptions` and **`components`**.

### `enable_graphql` (feature **`graphql`**)

Build an **async-graphql** `Schema` in your app (query/mutation/subscription types must implement the crate’s `ObjectType` / `SubscriptionType` traits), then pass it to **`NestFactory::create::<AppModule>().enable_graphql(schema)`** before **`listen`**. See **`nestrs-graphql`** and [GraphQL, WebSockets & microservices DX](graphql-ws-micro-dx.md).

---

## i18n and request-scoped DI

### `use_i18n`

Requires **`I18nService`** registration (typically via **`I18nModule`**). See [Ecosystem modules](ecosystem.md).

### `use_request_scope`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>().use_request_scope().into_router();
}
```

Pair with **`#[injectable(scope = "request")]`** and **`RequestScoped<T>`** ([Fundamentals](fundamentals.md)).

---

## Global layers and exception filter

### `use_global_layer`

```rust,noplayground
use axum::http::{HeaderName, HeaderValue};
use nestrs::prelude::*;
use tower_http::set_header::SetResponseHeaderLayer;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .use_global_layer(|router| {
            router.layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("x-app"),
                HeaderValue::from_static("demo"),
            ))
        })
        .into_router();
}
```

### `use_global_exception_filter`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

#[derive(Clone, Default)]
struct MyGlobalFilter;

#[async_trait::async_trait]
impl ExceptionFilter for MyGlobalFilter {
    async fn catch(
        &self,
        ex: HttpException,
    ) -> axum::response::Response {
        ex.into_response()
    }
}

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .use_global_exception_filter(MyGlobalFilter)
        .into_router();
}
```

### `enable_default_fallback`

```rust,noplayground
use nestrs::prelude::*;

#[module]
struct AppModule;

fn main() {
    let _router = NestFactory::create::<AppModule>()
        .enable_default_fallback()
        .into_router();
}
```

---

## Per-route: guards, interceptors, filters, pipes

From **`nestrs/tests/cross_cutting_ordering_contract.rs`** (simplified):

```rust,noplayground
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[derive(Default)]
struct MyGuard;

#[async_trait::async_trait]
impl CanActivate for MyGuard {
    async fn can_activate(
        &self,
        _parts: &axum::http::request::Parts,
    ) -> Result<(), GuardError> {
        Ok(())
    }
}

#[derive(Default)]
struct MyInterceptor;

#[async_trait::async_trait]
impl Interceptor for MyInterceptor {
    async fn intercept(
        &self,
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        next.run(req).await
    }
}

#[derive(Default)]
struct MyFilter;

#[async_trait::async_trait]
impl ExceptionFilter for MyFilter {
    async fn catch(
        &self,
        ex: HttpException,
    ) -> axum::response::Response {
        ex.into_response()
    }
}

#[controller(prefix = "/demo", version = "v1")]
struct DemoController;

#[routes(state = AppState)]
impl DemoController {
    #[get("/g")]
    #[use_guards(MyGuard)]
    async fn guarded() -> &'static str {
        "ok"
    }

    #[get("/i")]
    #[use_interceptors(MyInterceptor)]
    async fn intercepted() -> &'static str {
        "ok"
    }

    #[get("/f")]
    #[use_filters(MyFilter)]
    async fn filtered() -> Result<&'static str, HttpException> {
        Ok("ok")
    }
}

#[module(controllers = [DemoController], providers = [AppState])]
struct AppModule;
```

**`#[use_pipes(ValidationPipe)]`** with **`#[param::body]`** / DTOs: [Custom decorators](custom-decorators.md).

---

## CLI (`nestrs` / `nestrs-scaffold`)

```bash
cargo install nestrs-scaffold

nestrs new my-api
cd my-api && cargo run

nestrs doctor

nestrs generate service users/UserService --path src
nestrs g controller health/HealthController --path src
nestrs g module billing/BillingModule --path src
nestrs g guard auth/AuthGuard --path src
nestrs g dto items/ItemDto --path src

nestrs g resource orders --transport rest --path src
```

See [CLI](cli.md).

---

## Related

- [First steps](first-steps.md) — minimal app  
- [Fundamentals](fundamentals.md) — DI, `ModuleRef`, dynamic modules  
- [HTTP pipeline order](http-pipeline-order.md) — ordering rules  
- [Secure defaults](secure-defaults.md) — hardening checklist  
- [Observability](observability.md) — tracing + metrics golden path  
