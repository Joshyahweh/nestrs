//! Wave 3E.2 — health probe decorators (`#[liveness]` / `#[readiness]` /
//! `#[startup]`) and the standard indicator set.
//!
//! Each test builds a fresh `NestApplication` and exercises the fixed
//! `/__nestrs/health/{live,ready,startup}` endpoints via
//! `tower::ServiceExt::oneshot`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

static STARTUP_HITS: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
#[injectable]
struct ProbeAppState;

#[controller(prefix = "/internal")]
struct ProbeController;

#[routes(state = ProbeAppState)]
impl ProbeController {
    #[get("/alive")]
    #[liveness]
    async fn alive() -> &'static str {
        "alive"
    }

    #[get("/broken")]
    #[readiness]
    async fn broken() -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }

    #[get("/boot")]
    #[startup]
    async fn boot() -> StatusCode {
        if STARTUP_HITS.fetch_add(1, Ordering::SeqCst) == 0 {
            StatusCode::OK
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[module(controllers = [ProbeController], providers = [ProbeAppState])]
struct ProbeModule;

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

fn probe_app() -> axum::Router {
    NestFactory::create::<ProbeModule>().into_router()
}

#[tokio::test]
async fn liveness_mirror_reports_stamped_handler_status() {
    let app = probe_app();
    // `#[liveness]` on `/internal/alive` (2xx) ⇒ live endpoint up.
    let r = get(&app, "/__nestrs/health/live").await;
    assert_eq!(r.status(), StatusCode::OK);
    let body = axum::body::to_bytes(r.into_body(), 1024)
        .await
        .expect("body");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(v["status"], "ok");
}

#[tokio::test]
async fn readiness_mirror_surfaces_handler_failure_as_503() {
    let app = probe_app();
    // `#[readiness]` on `/internal/broken` (500) ⇒ ready endpoint 503 with
    // a generic message: the probe endpoints are unauthenticated, so the
    // failing path/status goes to `tracing`, never into the response.
    let r = get(&app, "/__nestrs/health/ready").await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = axum::body::to_bytes(r.into_body(), 4096)
        .await
        .expect("body");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(v["status"], "error");
    let msg = v["message"].as_str().expect("message");
    assert_eq!(msg, "readiness check failed", "generic message: {msg}");
    assert!(
        !msg.contains("/internal/broken") && !msg.contains("500"),
        "internal route topology must not leak into probe responses: {msg}"
    );
}

#[tokio::test]
async fn startup_probe_evaluates_exactly_once_per_process() {
    let app = probe_app();
    // First call runs the handler (OK) and caches the outcome.
    let r1 = get(&app, "/__nestrs/health/startup").await;
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(STARTUP_HITS.load(Ordering::SeqCst), 1, "ran once");
    // Subsequent calls return the cached outcome without re-running the
    // handler — even though a fresh run would now report 500.
    let r2 = get(&app, "/__nestrs/health/startup").await;
    assert_eq!(r2.status(), StatusCode::OK, "cached OK is served");
    assert_eq!(STARTUP_HITS.load(Ordering::SeqCst), 1, "still ran once");
}

#[tokio::test]
async fn probe_stamped_routes_are_ordinary_routes_too() {
    let app = probe_app();
    // The stamped handlers remain regular endpoints.
    let r = get(&app, "/internal/alive").await;
    assert_eq!(r.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Indicators
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StaticPing {
    fail: bool,
}

#[async_trait::async_trait]
impl nestrs::core::DatabasePing for StaticPing {
    async fn ping_database(&self) -> Result<(), String> {
        if self.fail {
            Err("connection refused".to_string())
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn database_indicator_reports_ping_outcome() {
    let up = nestrs::DatabaseIndicator::new(Arc::new(StaticPing { fail: false }));
    assert!(matches!(up.check().await, nestrs::HealthStatus::Up));

    let down = nestrs::DatabaseIndicator::new(Arc::new(StaticPing { fail: true }));
    match down.check().await {
        nestrs::HealthStatus::Down { message } => {
            assert!(message.contains("connection refused"), "{message}");
        }
        other => panic!("expected Down, got {other:?}"),
    }
}

#[cfg(feature = "http-client")]
#[tokio::test]
async fn http_indicator_up_and_down_paths() {
    use std::time::Duration;

    // A one-shot HTTP server on an ephemeral port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let Ok((sock, _)) = listener.accept().await else {
            return;
        };
        let mut sock = sock;
        use tokio::io::AsyncWriteExt;
        let _ = sock
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    });

    let up = nestrs::HttpIndicator::new(format!("http://{addr}/health"), Duration::from_secs(2));
    assert!(matches!(up.check().await, nestrs::HealthStatus::Up));

    // Nothing listening on the handshake port of a second bind-free addr —
    // point at an unroutable loopback port instead.
    let down = nestrs::HttpIndicator::new("http://127.0.0.1:1/health", Duration::from_millis(300));
    assert!(matches!(
        down.check().await,
        nestrs::HealthStatus::Down { .. }
    ));
}

#[cfg(all(unix, feature = "health-disk"))]
#[tokio::test]
async fn disk_indicator_threshold_semantics() {
    use std::time::Duration;
    // Tiny threshold on any writable dir ⇒ Up.
    let up = nestrs::DiskSpaceIndicator::new("/tmp", 1);
    assert!(matches!(up.check().await, nestrs::HealthStatus::Up));
    // Absurd threshold ⇒ Down.
    let down = nestrs::DiskSpaceIndicator::new("/tmp", u64::MAX);
    match down.check().await {
        nestrs::HealthStatus::Down { message } => {
            assert!(message.contains("free space"), "{message}");
        }
        other => panic!("expected Down, got {other:?}"),
    }
    let _ = Duration::from_secs(0); // keep import if cfgs diverge
}
