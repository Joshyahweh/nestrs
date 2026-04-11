# nestrs ecosystem modules

This repo aims for a NestJS-like developer experience in Rust. Some optional "ecosystem modules"
are implemented; others are intentionally staged for later phases.

## Status

- **CacheModule**: implemented (in-memory + optional Redis backend)
- **ScheduleModule**: implemented (feature `schedule`)
- **QueuesModule**: implemented (feature `queues`, in-process baseline)
- **I18nModule**: implemented (catalogs + locale resolver; enable via `NestApplication::use_i18n()`)

## CacheModule

Exports `CacheService` with basic `get/set/del/ttl` operations and optional TTL.

Example:

```rust
use nestrs::prelude::*;

#[module(
  imports = [CacheModule::register(CacheOptions::in_memory())],
  providers = [AppState],
  controllers = [AppController]
)]
struct AppModule;

#[injectable]
struct AppState {
    cache: std::sync::Arc<CacheService>,
}

#[controller(prefix = "/cache")]
struct AppController;

#[routes(state = AppState)]
impl AppController {
    #[get("/")]
    async fn read(State(state): State<std::sync::Arc<AppState>>) -> String {
        state
            .cache
            .get_json("hello")
            .await
            .unwrap_or(serde_json::Value::Null)
            .to_string()
    }
}
```

Redis backend (feature `cache-redis`):

```rust
use nestrs::prelude::*;

#[module(imports = [CacheModule::register(CacheOptions::redis("redis://localhost:6379"))])]
struct AppModule;
```

## ScheduleModule

Feature: `schedule`

```rust
use nestrs::prelude::*;

#[module(imports = [ScheduleModule::for_root()], providers = [TasksService])]
struct AppModule;

#[injectable]
struct TasksService;

#[schedule_routes]
impl TasksService {
    #[cron("0 * * * * *")]
    async fn run_every_minute(&self) {
        // ...
    }

    #[interval(30_000)]
    async fn run_every_30s(&self) {
        // ...
    }
}
```

## QueuesModule

Feature: `queues` (in-process baseline; Bull-ish API)

```rust
use nestrs::prelude::*;

#[module(
  imports = [QueuesModule::register(&[QueueConfig::new("EMAIL").with_concurrency(2)])],
  providers = [EmailProcessor],
)]
struct AppModule;

#[injectable]
#[queue_processor("EMAIL")]
struct EmailProcessor;

#[async_trait]
impl QueueHandler for EmailProcessor {
    async fn process(&self, job: QueueJob) -> Result<(), QueueError> {
        // match job.name / job.payload
        Ok(())
    }
}
```

Enqueue jobs by injecting `QueuesService` and calling `expect_queue("...").add(...)`.

## I18nModule

```rust
use nestrs::prelude::*;

#[module(imports = [I18nModule], providers = [AppState], controllers = [AppController])]
struct AppModule;

// Enable middleware on the HTTP app:
// NestFactory::create::<AppModule>().use_i18n().listen(3000).await;
```

The `Locale` / `I18n` extractors resolve locale from `?lang=...` (default) or `Accept-Language`, then translate via catalogs registered in `I18nService` / `I18nModule::register(I18nOptions { ... })`.

- **ScheduleModule**: likely backed by `tokio-cron-scheduler`, providing `#[cron]` and `#[interval]`
  decorators and a provider that runs jobs on application bootstrap.
- **QueuesModule**: likely backed by Redis and a worker runtime (jobs + retries + DLQ).
- **I18nModule**: locale detection (header/query/cookie), message bundles, and interpolation.

