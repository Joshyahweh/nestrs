# nestrs-http

Outbound HTTP client for the [nestrs](https://crates.io/crates/nestrs) framework — the Rust equivalent of [`@nestjs/axios`](https://docs.nestjs.com/techniques/http-module).

```rust,ignore
use nestrs_http::{HttpModule, HttpService};

#[module(imports = [HttpModule::register()])]
struct AppModule;

async fn call_external(svc: &HttpService) -> Result<(), reqwest::Error> {
    let body: serde_json::Value = svc
        .get("https://api.example.com/users")
        .send()
        .await?
        .json()
        .await?;
    Ok(())
}
```

## What you get

- `HttpModule::register()` — installs a singleton `HttpService` provider.
- `HttpService` — wraps a shared `reqwest::Client` with sane default timeouts (30s whole-request, 10s connect). `.get(url)`, `.post(url)`, `.put(url)`, `.patch(url)`, `.delete(url)` return `reqwest::RequestBuilder`s; `.client()` exposes the raw client for advanced use.
- `HttpServiceOptions` — `request_timeout` / `connect_timeout` builder.
- `DEFAULT_REQUEST_TIMEOUT` / `DEFAULT_CONNECT_TIMEOUT` — the docs / tests can assert these.
- `reqwest` re-exported at the crate root under feature flag `reqwest` so callers don't add a direct dep.

## Why

reqwest has no default timeout. Without an outbound deadline, a TCP-connected but unresponsive upstream hangs the calling handler's future forever and concurrent hung calls accumulate until the runtime is saturated. `HttpService` enforces a 30s / 10s default — tunable per-deployment via `HttpServiceOptions`.

## Feature flags

- `default = []` — base crate.
- `reqwest` — re-exports `reqwest` at the crate root.

## License

MIT OR Apache-2.0.