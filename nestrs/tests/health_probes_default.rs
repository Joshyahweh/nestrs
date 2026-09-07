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
