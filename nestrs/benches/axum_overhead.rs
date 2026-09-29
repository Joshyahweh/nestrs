//! Same `GET /ping` -> `ok` handler, three routers, one process.
//!
//! Measures the cost of nestrs on top of Axum. There is no socket:
//! each iteration is `Router::oneshot`, matching the other router benches.
//!
//! * `axum_get_ping` — bare `axum::Router`, no limit or catch-panic layer.
//! * `nestrs_get_ping_stack_off` — `NestFactory::create().into_router()` with
//!   no `use_*` calls. That default still mounts the body-size limit,
//!   `CatchPanicLayer`, and the built-in health-probe routes.
//! * `nestrs_get_ping_stack_on` — the same app plus request id, request
//!   context, request tracing, compression, and a concurrency limit of 128.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use criterion::{criterion_group, criterion_main, Criterion};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct BenchState;

#[controller(prefix = "/")]
struct BenchController;

impl BenchController {
    #[get("/ping")]
    async fn ping() -> &'static str {
        "ok"
    }
}

impl_routes!(BenchController, state BenchState => [
    GET "/ping" with () => BenchController::ping,
]);

#[module(
    controllers = [BenchController],
    providers = [BenchState],
)]
struct BenchModule;

fn ping_request() -> Request<Body> {
    Request::builder()
        .uri("/ping")
        .method("GET")
        .body(Body::empty())
        .expect("request")
}

async fn oneshot_ping(router: Router) {
    let _res = router.oneshot(ping_request()).await.expect("response");
}

fn assert_ok(rt: &tokio::runtime::Runtime, router: Router) {
    rt.block_on(async {
        let response = router.oneshot(ping_request()).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64)
            .await
            .expect("body");
        assert_eq!(bytes.as_ref(), b"ok");
    });
}

fn bench_axum_overhead(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    let axum_router = Router::new().route("/ping", get(|| async { "ok" }));
    let nestrs_off = NestFactory::create::<BenchModule>().into_router();
    let nestrs_on = NestFactory::create::<BenchModule>()
        .use_request_id()
        .use_request_context()
        .use_request_tracing(RequestTracingOptions::builder())
        .use_compression()
        .use_concurrency_limit(128)
        .into_router();

    assert_ok(&rt, axum_router.clone());
    assert_ok(&rt, nestrs_off.clone());
    assert_ok(&rt, nestrs_on.clone());

    c.bench_function("axum_get_ping", |b| {
        b.to_async(&rt).iter(|| oneshot_ping(axum_router.clone()))
    });
    c.bench_function("nestrs_get_ping_stack_off", |b| {
        b.to_async(&rt).iter(|| oneshot_ping(nestrs_off.clone()))
    });
    c.bench_function("nestrs_get_ping_stack_on", |b| {
        b.to_async(&rt).iter(|| oneshot_ping(nestrs_on.clone()))
    });
}

criterion_group!(benches, bench_axum_overhead);
criterion_main!(benches);
