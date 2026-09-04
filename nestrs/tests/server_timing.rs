//! `Server-Timing` middleware (RFC 8628) integration tests.
//!
//!  * `server_timing_emits_header_on_ok_response` — basic presence of `Server-Timing: total;dur=…`
//!  * `server_timing_records_per_request_named_timer_via_extractor` — handler-recorded `db` entry
//!    appears alongside `total`
//!  * `server_timing_skips_header_when_below_min_ms_threshold` — `min_ms_to_report = 1000`
//!    suppresses the header entirely for sub-millisecond requests

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use std::time::Duration;
use tower::util::ServiceExt;

// -- Test app ---------------------------------------------------------------

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/timed")]
struct TimedController;

#[routes(state = AppState)]
impl TimedController {
    #[get("/hello")]
    async fn hello() -> &'static str {
        "hi"
    }

    #[get("/db")]
    async fn db(timing: ServerTiming) -> &'static str {
        timing.start("db");
        // tiny sleep so the timer is non-zero
        tokio::time::sleep(Duration::from_millis(2)).await;
        timing.stop("db");
        "db-ok"
    }
}

#[module(providers = [AppState], controllers = [TimedController])]
struct AppModule;

#[module(providers = [AppState], controllers = [TimedController])]
struct AppModuleHighThreshold;

fn build_router_default() -> axum::Router {
    NestFactory::create::<AppModule>()
        .use_server_timing()
        .into_router()
}

fn build_router_high_threshold() -> axum::Router {
    NestFactory::create::<AppModuleHighThreshold>()
        .use_server_timing_with(ServerTimingConfig {
            min_ms_to_report: 1000,
        })
        .into_router()
}

// -- Tests ------------------------------------------------------------------

#[tokio::test]
async fn server_timing_emits_header_on_ok_response() {
    let router = build_router_default();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/timed/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
    let header = res
        .headers()
        .get("server-timing")
        .expect("Server-Timing header present")
        .to_str()
        .expect("ascii")
        .to_string();
    assert!(
        header.contains("total;dur="),
        "expected `total;dur=…` in header, got `{header}`"
    );
    // Sanity: body still went through.
    let body = to_bytes(res.into_body(), 1024).await.expect("body");
    assert_eq!(std::str::from_utf8(&body).expect("utf8"), "hi");
}

#[tokio::test]
async fn server_timing_records_per_request_named_timer_via_extractor() {
    let router = build_router_default();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/timed/db")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
    let header = res
        .headers()
        .get("server-timing")
        .expect("Server-Timing header present")
        .to_str()
        .expect("ascii")
        .to_string();
    assert!(
        header.contains("db;dur="),
        "expected `db;dur=…` in header, got `{header}`"
    );
    assert!(
        header.contains("total;dur="),
        "expected `total;dur=…` in header, got `{header}`"
    );
    // `db` should be listed before or after `total` but both present.
    let body = to_bytes(res.into_body(), 1024).await.expect("body");
    assert_eq!(std::str::from_utf8(&body).expect("utf8"), "db-ok");
}

#[tokio::test]
async fn server_timing_skips_header_when_below_min_ms_threshold() {
    // Handler is a no-op; request should take << 1ms locally, so threshold=1000 filters it.
    let router = build_router_high_threshold();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/timed/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers().get("server-timing").is_none(),
        "header must be omitted when total is below min_ms_to_report, but got {:?}",
        res.headers().get("server-timing")
    );
}
