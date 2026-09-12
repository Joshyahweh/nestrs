# Security

nestrs surfaces **security-sensitive behavior** through explicit APIs (CSRF, CORS, cookies, sessions, headers) and **runtime warnings** when a production-shaped environment variable set suggests a risky combination. The long-form policy, disclosure process, and control descriptions live in the repository’s **`SECURITY.md`**, included below.

## Quick checklist (before the full policy)

1. **Cookies + mutations**: If you use cookie sessions in browsers, pair them with a CSRF strategy—see [Secure defaults](secure-defaults.md) and the `csrf` feature.  
2. **CORS**: Do not ship permissive `*` origins in production; use environment-specific allowlists.  
3. **Headers**: Enable `use_security_headers` (or equivalent) for browser-facing APIs.  
4. **Errors**: Production error sanitization is **on by default** when `NESTRS_ENV`/`APP_ENV`/`RUST_ENV` is `production`/`prod` — 5xx `message` becomes a generic string and `errors` is dropped. `disable_production_errors()` opts out only behind a trusted boundary.  
5. **Rate limiting + throttling**: `use_rate_limit(...)` globally, `use_throttler(...)` + `#[throttle(n, "minute")]` / `#[skip_throttle]` per route. Rejected routes answer `429` with `Retry-After` and `X-RateLimit-Remaining`.  
6. **Proxy topology**: `use_trusted_proxy_headers(hops)` must match the real proxy chain so rate limits key on true client IPs (0 if directly exposed).  
7. **Dependencies**: Run `cargo audit` (or your org’s scanner) on a schedule; nestrs CI includes security workflows you can mirror locally.  

For **OpenAPI security schemes** and **`#[roles]`** documentation hints, see [OpenAPI & HTTP](openapi-http.md). For **`use_cookies`**, **`use_csrf_protection`**, **`use_security_headers`**, and related **`NestApplication`** snippets, see the [API cookbook](appendix-api-cookbook.md).

## Route-level throttling

Where the global rate limiter applies one budget to everything, **`use_throttler`** applies per-route budgets handlers declare with decorators:

```rust
NestFactory::create::<AppModule>()
    .use_throttler(
        ThrottlerOptions::builder()
            .global(ThrottleSpec::parse("100/minute")) // fallback for undecorated routes
            .build(),
    )
    .listen(3000)
    .await;
```

```rust
#[routes(state = AppState)]
impl UploadController {
    #[post("/avatar")]
    #[throttle(5, "minute")]        // 5 uploads per client per minute
    async fn avatar(/* … */) { /* … */ }

    #[get("/status")]
    #[skip_throttle]                // exempt entirely
    async fn status() -> &'static str { "ok" }
}
```

- **`#[throttle(n, "second" | "minute" | "hour")]`** overrides the global spec for that route.
- **`ThrottlerOptions.global`** is the fallback for undecorated routes — `None` means only decorated routes are throttled.
- Shared Redis backend via the **`cache-redis`** feature; inherits the trusted-proxy hop count (below).

## Proxy topology and client identity

The rate limiter and throttler key on the **client IP**. Behind a proxy/LB the socket address is the proxy's, so identity must come from `X-Forwarded-For` / `X-Real-IP` — attacker-spoofable, so nestrs never trusts them unless you declare the topology:

```rust
NestFactory::create::<AppModule>()
    .use_trusted_proxy_headers(1)   // exactly one trusted proxy in front
    .use_rate_limit(RateLimitOptions::builder().max_requests(200).window_secs(60).build())
    .listen(3000)
    .await;
```

- **`use_trusted_proxy_headers(hops)`** declares how many trusted proxies sit in front. The client address is read **right-most-first**: with `client → LB → app`, the LB-appended entry is used and any attacker-supplied prefix is ignored.
- Forwarded headers are **never** consulted without this call — defaulting to trust would allow trivial rate-limit bypass via header forgery.
- The rate limiter and throttler **inherit** the hop count automatically; divergent per-component overrides log a `tracing` WARN.
- Topologies: direct exposure → omit the call (0 hops); one load balancer → `1`; LB + CDN → `2` (count only hops you control).

---

{{#include ../../SECURITY.md}}

