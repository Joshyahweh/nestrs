# nestrs-lambda

Run a nestrs Axum router on AWS Lambda. NestJS uses platform adapters for
Express/Fastify on Lambda; nestrs is Axum-only (ADR-0001), so this crate is
the serverless adapter.

```toml
nestrs-lambda = "1.4.0"
```

```rust,ignore
use nestrs::NestFactory;
use nestrs_lambda::listen_lambda;

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    let router = NestFactory::create::<AppModule>().into_router();
    listen_lambda(router).await
}
```
