//! Integration tests for `#[throttle]` / `#[skip_throttle]` decorators,
//! `ThrottlerModule` / `use_throttler`, and the `ThrottlerGuard` shape.
//!
//! Each test builds a fresh `NestApplication` and exercises real middleware
//! via `tower::ServiceExt::oneshot`. Redis cross-instance tests are gated on
//! `NESTRS_TEST_REDIS_URL` (no mini-redis in CI).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use nestrs::prelude::*;
use nestrs::{
    skip_throttle, throttle, ThrottleSpec, ThrottlerGuard, ThrottlerModule, ThrottlerOptions,
    ThrottlerService,
};
// Trait import: `RedisThrottler::check` below needs it in scope, but only the
// `cache-redis`-gated test uses it — importing it unconditionally trips
// unused-imports in builds without that feature.
#[cfg(feature = "cache-redis")]
use nestrs::ThrottlerBackend;
use std::sync::Arc;
use tower::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

// ---------------------------------------------------------------------------
// Decorator-driven throttling (middleware path)
// ---------------------------------------------------------------------------

#[controller(prefix = "/api")]
struct DecoratedController;

#[routes(state = AppState)]
impl DecoratedController {
    #[get("/limited")]
    #[throttle(2, "minute")]
    async fn limited() -> &'static str {
        "limited-ok"
    }

    #[get("/exempt")]
    #[skip_throttle]
    async fn exempt() -> &'static str {
        "exempt-ok"
    }

    #[get("/plain")]
    async fn plain() -> &'static str {
        "plain-ok"
    }
}

#[module(controllers = [DecoratedController], providers = [AppState])]
struct DecoratedModule;

#[derive(Default)]
struct AllowAllGuard;

#[async_trait::async_trait]
impl CanActivate for AllowAllGuard {
    async fn can_activate(&self, _parts: &axum::http::request::Parts) -> Result<(), GuardError> {
        Ok(())
    }
}

#[controller(prefix = "/g")]
struct GuardedController;

#[routes(state = AppState)]
impl GuardedController {
    #[get("/limited")]
    #[throttle(1, "minute")]
    #[use_guards(ThrottlerGuard, AllowAllGuard)]
    async fn guarded() -> &'static str {
        "guarded-ok"
    }
}

#[module(
    controllers = [GuardedController],
    providers = [AppState, ThrottlerService],
    imports = [ThrottlerModule::register(ThrottlerOptions { global: Some(ThrottleSpec { limit: 100, window_secs: 60 }), ..ThrottlerOptions::default() })]
)]
struct GuardedModule;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn get(app: &axum::Router, path: &str) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
}

/// `get` with a single-entry `x-forwarded-for` chain — the entry a trusted
/// proxy appends for its directly-connected client.
async fn get_xff(app: &axum::Router, path: &str, client: &str) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(path)
                .header("x-forwarded-for", client)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
}

fn app_with_global_decorated(global: Option<ThrottleSpec>) -> axum::Router {
    let mut app = NestFactory::create::<DecoratedModule>();
    if let Some(g) = global {
        app = app.use_throttler(ThrottlerOptions {
            global: Some(g),
            ..ThrottlerOptions::default()
        });
    }
    app.into_router()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn per_route_decorator_admits_then_429s() {
    // Decorator metadata is inert until the throttler middleware is
    // installed — `use_throttler` with `global: None` enforces ONLY the
    // `#[throttle]`-decorated routes (the undecorated `/api/plain` route
    // stays open; see `no_options_disables_throttling_entirely` for the
    // fully-inert case).
    let app = NestFactory::create::<DecoratedModule>()
        .use_throttler(ThrottlerOptions {
            global: None,
            ..ThrottlerOptions::default()
        })
        .into_router();
    let r1 = get(&app, "/api/limited").await;
    let r2 = get(&app, "/api/limited").await;
    let r3 = get(&app, "/api/limited").await;
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::OK);
    assert_eq!(r3.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = r3
        .headers()
        .get("retry-after")
        .expect("retry-after")
        .to_str()
        .expect("ascii")
        .parse()
        .expect("u64");
    assert!(
        (1..=60).contains(&retry_after),
        "minute window ⇒ retry-after within 60s, got {retry_after}"
    );
    assert_eq!(
        r3.headers()
            .get("x-ratelimit-remaining")
            .expect("x-ratelimit-remaining"),
        "0"
    );
    assert_eq!(
        r3.headers()
            .get("x-ratelimit-limit")
            .expect("x-ratelimit-limit"),
        "2"
    );
}

#[tokio::test]
async fn skip_throttle_exempts_route() {
    let app = app_with_global_decorated(Some(ThrottleSpec {
        limit: 1,
        window_secs: 60,
    }));
    for _ in 0..5 {
        let r = get(&app, "/api/exempt").await;
        assert_eq!(r.status(), StatusCode::OK);
    }
    // Sibling `plain` route still throttled by the global spec.
    let r1 = get(&app, "/api/plain").await;
    let r2 = get(&app, "/api/plain").await;
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn per_route_decorator_overrides_global_spec() {
    let app = app_with_global_decorated(Some(ThrottleSpec {
        limit: 100,
        window_secs: 60,
    }));
    // Global would admit 100; per-route limit 2 rejects 3rd.
    let r1 = get(&app, "/api/limited").await;
    let r2 = get(&app, "/api/limited").await;
    let r3 = get(&app, "/api/limited").await;
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::OK);
    assert_eq!(r3.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r3.headers().get("x-ratelimit-limit").expect("limit"), "2");
}

#[tokio::test]
async fn no_options_disables_throttling_entirely() {
    let app = app_with_global_decorated(None);
    for _ in 0..5 {
        let r = get(&app, "/api/limited").await;
        assert_eq!(r.status(), StatusCode::OK, "no-op without use_throttler");
    }
}

#[tokio::test]
async fn module_register_publishes_throttler_service() {
    let module = ThrottlerModule::register(ThrottlerOptions {
        global: Some(ThrottleSpec {
            limit: 5,
            window_secs: 60,
        }),
        ..ThrottlerOptions::default()
    });
    let service: Arc<ThrottlerService> = module.registry.get::<ThrottlerService>();
    let outcome = service
        .check(
            "k1",
            &ThrottleSpec {
                limit: 1,
                window_secs: 60,
            },
        )
        .await;
    assert!(matches!(outcome, nestrs::ThrottleOutcome::Allowed { .. }));
    let outcome = service
        .check(
            "k1",
            &ThrottleSpec {
                limit: 1,
                window_secs: 60,
            },
        )
        .await;
    assert!(matches!(outcome, nestrs::ThrottleOutcome::Limited { .. }));
}

#[tokio::test]
async fn throttler_guard_rejects_via_guard_error() {
    let app = NestFactory::create::<GuardedModule>().into_router();
    let r1 = get(&app, "/g/limited").await;
    let r2 = get(&app, "/g/limited").await;
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = axum::body::to_bytes(r2.into_body(), 1024)
        .await
        .expect("body");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(v["statusCode"], 429);
    assert_eq!(v["error"], "Too Many Requests");
}

#[tokio::test]
async fn throttler_inherits_app_trusted_proxy_hops_by_default() {
    // One declared proxy (`use_trusted_proxy_headers(1)`): distinct
    // `x-forwarded-for` clients must land in distinct throttle buckets even
    // though every oneshot request shares the same (absent) ConnectInfo.
    // Before inheritance the middleware ignored the app topology, keyed
    // every request as `unknown`, and collectively 429'd unrelated clients.
    let app = NestFactory::create::<DecoratedModule>()
        .use_trusted_proxy_headers(1)
        .use_throttler(ThrottlerOptions {
            global: None,
            ..ThrottlerOptions::default()
        })
        .into_router();
    // `#[throttle(2, "minute")]` on /api/limited.
    let r1 = get_xff(&app, "/api/limited", "203.0.113.10").await;
    let r2 = get_xff(&app, "/api/limited", "203.0.113.10").await;
    let r3 = get_xff(&app, "/api/limited", "203.0.113.10").await;
    let r4 = get_xff(&app, "/api/limited", "203.0.113.11").await;
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::OK);
    assert_eq!(
        r3.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "budget of 2 exhausted for 203.0.113.10"
    );
    assert_eq!(
        r4.status(),
        StatusCode::OK,
        "a different client behind the same proxy gets its own bucket"
    );
}

#[tokio::test]
async fn throttler_explicit_hops_override_app_topology() {
    // `trusted_proxy_hops: Some(0)` is a deliberate override: even though the
    // app declares one proxy, the throttler keys on connection metadata only
    // — here one shared `unknown` bucket, so the second distinct client 429s.
    // (A divergence warning is logged for this shape.)
    let app = NestFactory::create::<DecoratedModule>()
        .use_trusted_proxy_headers(1)
        .use_throttler(ThrottlerOptions {
            global: None,
            trusted_proxy_hops: Some(0),
            ..ThrottlerOptions::default()
        })
        .into_router();
    let r1 = get_xff(&app, "/api/limited", "203.0.113.10").await;
    let r2 = get_xff(&app, "/api/limited", "203.0.113.10").await;
    let r3 = get_xff(&app, "/api/limited", "203.0.113.11").await;
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::OK);
    assert_eq!(
        r3.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "explicit 0 means forwarded headers are ignored: one shared bucket"
    );
}

#[tokio::test]
async fn throttler_guard_reads_proxy_topology_from_request() {
    // The guard runs at route level — inside the trusted-proxy middleware —
    // so it inherits the per-request hop count instead of hardcoding 0.
    // `#[throttle(1, "minute")]` on /g/limited: each resolved client gets a
    // budget of one.
    let app = NestFactory::create::<GuardedModule>()
        .use_trusted_proxy_headers(1)
        .into_router();
    let r1 = get_xff(&app, "/g/limited", "203.0.113.10").await;
    let r2 = get_xff(&app, "/g/limited", "203.0.113.20").await;
    let r3 = get_xff(&app, "/g/limited", "203.0.113.20").await;
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(
        r2.status(),
        StatusCode::OK,
        "a different client behind the proxy has its own guard bucket"
    );
    assert_eq!(
        r3.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "budget of 1 exhausted for 203.0.113.20"
    );
}

#[cfg(feature = "cache-redis")]
#[tokio::test]
async fn redis_backend_shares_budget_across_instances() {
    let Some(url) = std::env::var("NESTRS_TEST_REDIS_URL").ok() else {
        eprintln!("NESTRS_TEST_REDIS_URL unset; skipping");
        return;
    };
    let prefix = format!("test-{}", std::process::id());
    let a = nestrs::RedisThrottler::new(&url, &prefix).expect("client a");
    let b = nestrs::RedisThrottler::new(&url, &prefix).expect("client b");
    let spec = ThrottleSpec {
        limit: 1,
        window_secs: 60,
    };
    // Drain via A
    let r1 = a.check("shared", &spec).await;
    let r2 = a.check("shared", &spec).await;
    assert!(matches!(r1, nestrs::ThrottleOutcome::Allowed { .. }));
    assert!(matches!(r2, nestrs::ThrottleOutcome::Limited { .. }));
    // B sees the same counter
    let r3 = b.check("shared", &spec).await;
    assert!(matches!(r3, nestrs::ThrottleOutcome::Limited { .. }));
}
