//! Wave 3E.2 — probe endpoints with **no** stamped handlers: both probe
//! kinds default to up, and the readiness endpoint aggregates the
//! `enable_readiness_check` indicators.
//!
//! This is a separate test binary from `health_probes.rs` on purpose: probe
//! stamps resolve through the process-global route + metadata registries,
//! so a binary containing stamped apps would leak its stamps into the
//! no-stamp assertions.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use std::sync::Arc;
use tower::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/plain")]
struct PlainController;

#[routes(state = AppState)]
impl PlainController {
    #[get("/ping")]
    async fn ping() -> &'static str {
        "ok"
    }
}

#[module(controllers = [PlainController], providers = [AppState])]
struct PlainModule;

async fn get(app: &axum::Router, path: &str) -> axum::response::Response {
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

#[tokio::test]
async fn probe_endpoints_exist_and_default_to_up() {
    let app = NestFactory::create::<PlainModule>().into_router();
    // NOTE: not the startup endpoint — the process-global startup cache is
    // single-shot and only tested where the stamped handler controls it.
    for path in ["/__nestrs/health/live", "/__nestrs/health/ready"] {
        let r = get(&app, path).await;
        assert_eq!(r.status(), StatusCode::OK, "{path} defaults to up");
        let body = axum::body::to_bytes(r.into_body(), 1024)
            .await
            .expect("body");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(v["status"], "ok", "{path} body shape");
    }
}

struct AlwaysDown;
#[async_trait::async_trait]
impl nestrs::HealthIndicator for AlwaysDown {
    fn name(&self) -> &'static str {
        "dep-x"
    }
    async fn check(&self) -> nestrs::HealthStatus {
        nestrs::HealthStatus::down("dependency down")
    }
}

#[tokio::test]
async fn readiness_endpoint_aggregates_indicators_when_no_handler_stamped() {
    let app = NestFactory::create::<PlainModule>()
        .enable_readiness_check(
            "/readyz",
            [Arc::new(AlwaysDown) as Arc<dyn nestrs::HealthIndicator>],
        )
        .into_router();

    // The fixed aggregation endpoint sees the app's indicators.
    let r = get(&app, "/__nestrs/health/ready").await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = axum::body::to_bytes(r.into_body(), 4096)
        .await
        .expect("body");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert!(
        v["message"].as_str().unwrap_or("").contains("dep-x"),
        "names the failing indicator: {v}"
    );
}

#[tokio::test]
async fn readiness_endpoint_up_when_all_indicators_healthy() {
    struct AlwaysUp;
    #[async_trait::async_trait]
    impl nestrs::HealthIndicator for AlwaysUp {
        fn name(&self) -> &'static str {
            "dep-ok"
        }
        async fn check(&self) -> nestrs::HealthStatus {
            nestrs::HealthStatus::Up
        }
    }

    let app = NestFactory::create::<PlainModule>()
        .enable_readiness_check(
            "/readyz",
            [Arc::new(AlwaysUp) as Arc<dyn nestrs::HealthIndicator>],
        )
        .into_router();

    let r = get(&app, "/__nestrs/health/ready").await;
    assert_eq!(r.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Hardening: panic guard, message redaction, execution coalescing
// (aggregation-path tests live in this no-stamp binary; the stamped-path
// panic test lives in `health_probes_panic.rs`)
// ---------------------------------------------------------------------------

struct PanickingIndicator;
#[async_trait::async_trait]
impl nestrs::HealthIndicator for PanickingIndicator {
    fn name(&self) -> &'static str {
        "boom-indicator"
    }
    async fn check(&self) -> nestrs::HealthStatus {
        panic!("indicator exploded");
    }
}

#[tokio::test]
async fn readiness_indicator_panic_returns_503_not_connection_drop() {
    // Probes mount outside CatchPanic, so a panicking indicator previously
    // unwound the connection task. The execution now runs on its own task:
    // the panic becomes a 503 with a generic message.
    let app = NestFactory::create::<PlainModule>()
        .enable_readiness_check(
            "/readyz",
            [Arc::new(PanickingIndicator) as Arc<dyn nestrs::HealthIndicator>],
        )
        .into_router();
    let r = get(&app, "/__nestrs/health/ready").await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = axum::body::to_bytes(r.into_body(), 1024)
        .await
        .expect("body");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(v["status"], "error");
    let msg = v["message"].as_str().expect("message");
    assert_eq!(msg, "probe execution failed");
    assert!(
        !msg.contains("indicator exploded"),
        "panic payload must not leak into probe responses: {msg}"
    );
}

struct LeakyIndicator;
#[async_trait::async_trait]
impl nestrs::HealthIndicator for LeakyIndicator {
    fn name(&self) -> &'static str {
        "dep-x"
    }
    async fn check(&self) -> nestrs::HealthStatus {
        nestrs::HealthStatus::down("connection refused to postgres://secret-host:5432")
    }
}

#[tokio::test]
async fn readiness_aggregation_redacts_indicator_error_text() {
    // Failing indicator NAMES ride in the message (operator's own static
    // labels); the raw error text — endpoints, hosts, DB error strings —
    // goes to `tracing` only.
    let app = NestFactory::create::<PlainModule>()
        .enable_readiness_check(
            "/readyz",
            [Arc::new(LeakyIndicator) as Arc<dyn nestrs::HealthIndicator>],
        )
        .into_router();
    let r = get(&app, "/__nestrs/health/ready").await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = axum::body::to_bytes(r.into_body(), 1024)
        .await
        .expect("body");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let msg = v["message"].as_str().expect("message");
    assert!(
        msg.contains("dep-x"),
        "failing indicator names stay in the message: {msg}"
    );
    assert!(
        !msg.contains("secret-host") && !msg.contains("postgres://"),
        "raw indicator error text must not leak into probe responses: {msg}"
    );
}

#[tokio::test]
async fn readiness_coalesces_concurrent_probes_into_one_execution() {
    // A probe storm must not amplify into the dependencies the indicators
    // call: a burst of concurrent probes shares one in-flight execution,
    // and the outcome is cached for the TTL window.
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CHECKS: AtomicUsize = AtomicUsize::new(0);

    struct SlowIndicator;
    #[async_trait::async_trait]
    impl nestrs::HealthIndicator for SlowIndicator {
        fn name(&self) -> &'static str {
            "slow-dep"
        }
        async fn check(&self) -> nestrs::HealthStatus {
            CHECKS.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            nestrs::HealthStatus::down("slow dependency down")
        }
    }

    let app = NestFactory::create::<PlainModule>()
        .enable_readiness_check(
            "/readyz",
            [Arc::new(SlowIndicator) as Arc<dyn nestrs::HealthIndicator>],
        )
        .into_router();

    let mut handles = Vec::new();
    for _ in 0..8 {
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            get(&app, "/__nestrs/health/ready").await
        }));
    }
    for h in handles {
        let r = h
            .await
            .expect("probe task must not fail: a panic in the guarded execution becomes a 503");
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    assert_eq!(
        CHECKS.load(Ordering::SeqCst),
        1,
        "a concurrent burst of 8 probes must execute the indicators exactly once"
    );

    // Within the TTL the cached outcome serves follow-ups without touching
    // the indicator again.
    let r = get(&app, "/__nestrs/health/ready").await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        CHECKS.load(Ordering::SeqCst),
        1,
        "cached outcome serves follow-up probes within the TTL"
    );
}
