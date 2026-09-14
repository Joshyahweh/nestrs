//! Integration tests for OTLP metrics + logs export (feature: `otel`).
//!
//! 1. The OTLP meter / logger pipelines install offline (exporters only
//!    connect on export, not on construction).
//! 2. Dual export: with `OpenTelemetryConfig::metrics()` **and**
//!    `enable_metrics`, the same framework RED instruments flow to the OTLP
//!    bridge *and* the Prometheus `/metrics` scraper — the fanout recorder
//!    hosts both as members, added in either order.
//! 3. `.logs()` bridges the `tracing` facade into the OTLP log pipeline
//!    without disturbing the fmt layer.

#![cfg(feature = "otel")]

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use nestrs::{try_init_tracing_opentelemetry, OpenTelemetryConfig, TracingConfig};
use serial_test::serial;
use tower::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller]
struct ProbeController;

#[routes(state = AppState)]
impl ProbeController {
    #[get("/probe")]
    async fn probe() -> &'static str {
        "probe-ok"
    }
}

#[module(controllers = [ProbeController], providers = [AppState])]
struct AppModule;

async fn get(router: &axum::Router, path: &str) -> axum::response::Response {
    router
        .clone()
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

async fn body_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

/// The OTLP meter + logger pipelines build without a collector: exporter
/// construction never dials the endpoint (tonic's `connect_lazy` defers the
/// connection to first export), so a missing collector must not fail
/// installation. Construction *does* require a Tokio reactor to be
/// current (the lazy channel task is spawned onto it) — same requirement
/// the existing tracer path has; real apps call this from `#[tokio::main]`.
#[tokio::test]
#[serial(otel_install)]
async fn otlp_meter_and_logger_install_offline() {
    let config = OpenTelemetryConfig::new("it-otel").endpoint("http://localhost:4317");

    let meter = nestrs::otel::install_otlp_meter(&config).expect("meter install");
    let counter = meter.u64_counter("it_otel_custom_total").build();
    counter.add(1, &[]);

    let provider = nestrs::otel::install_otlp_logger(&config).expect("logger install");
    drop(provider);

    // Shutdown paths must be safe to call (idempotent no-ops after install;
    // a refused localhost export surfaces as an ignored error, never a
    // panic).
    nestrs::otel::shutdown_meter_provider();
    nestrs::otel::shutdown_logger_provider();
    nestrs::otel::shutdown_tracer_provider();
}

/// Dual export: opt into OTLP metrics + logs, then enable the Prometheus
/// scraper — and confirm the framework RED metrics render at `/metrics`
/// after driving traffic through the middleware. This exercises the
/// real fanout: the OTLP bridge joins first, the Prometheus recorder
/// second, and every middleware recording fans out to both.
#[tokio::test]
#[serial(otel_install)]
async fn dual_export_prometheus_and_otel_side_by_side() {
    // OTLP opt-ins install the fanout recorder + the OTel bridge member.
    try_init_tracing_opentelemetry(
        TracingConfig::default(),
        OpenTelemetryConfig::new("it-otel")
            .endpoint("http://localhost:4317")
            .metrics()
            .logs(),
    )
    .expect("install tracing + otel");

    // The Prometheus recorder joins the fanout as a second member.
    let router = NestFactory::create::<AppModule>()
        .enable_metrics("/metrics")
        .into_router();

    let api = get(&router, "/probe").await;
    assert_eq!(api.status(), StatusCode::OK);

    let scrape = get(&router, "/metrics").await;
    assert_eq!(scrape.status(), StatusCode::OK);
    let text = body_text(scrape).await;
    assert!(text.contains("http_requests_total"), "{text}");
    assert!(text.contains("http_request_duration_seconds"), "{text}");
    assert!(text.contains("http_requests_in_flight"), "{text}");
    // The /probe request was recorded through the fanout — labeled by
    // method and status.
    assert!(text.contains("method=\"GET\""), "{text}");
    assert!(text.contains("status=\"200\""), "{text}");

    // A tracing event (the logs opt-in bridges these into the OTLP log
    // pipeline) must not disturb the fmt layer or panic mid-request.
    tracing::info!("dual-export smoke event");
}
