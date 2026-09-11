//! Probe hardening — a **panicking stamped handler** must yield a 503 probe
//! response, not a dropped connection.
//!
//! Separate binary from `health_probes.rs` on purpose: probe stamps resolve
//! through the process-global route + metadata registries, and this binary's
//! `#[readiness]` stamp (a panicking handler) would leak into the other
//! binary's stamped-route assertions.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::ServiceExt;

#[derive(Default)]
#[injectable]
struct PanicAppState;

#[controller(prefix = "/internal")]
struct PanicProbeController;

#[routes(state = PanicAppState)]
impl PanicProbeController {
    #[get("/explode")]
    #[readiness]
    async fn explode() -> &'static str {
        panic!("stamped handler exploded");
    }
}

#[module(controllers = [PanicProbeController], providers = [PanicAppState])]
struct PanicProbeModule;

#[tokio::test]
async fn panicking_stamped_readiness_handler_yields_503_not_connection_drop() {
    let app = NestFactory::create::<PanicProbeModule>().into_router();
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/__nestrs/health/ready")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("probe execution runs on its own task; a panic becomes a 503, never a dropped connection");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = axum::body::to_bytes(resp.into_body(), 1024)
        .await
        .expect("body");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(v["status"], "error");
    let msg = v["message"].as_str().expect("message");
    assert_eq!(msg, "readiness check failed");
    assert!(
        !msg.contains("/internal/explode") && !msg.contains("exploded"),
        "internal path and panic payload must not leak: {msg}"
    );
}
