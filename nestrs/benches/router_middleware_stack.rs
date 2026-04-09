use axum::body::Body;
use axum::http::Request;
use criterion::{criterion_group, criterion_main, Criterion};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct BenchState;

#[controller(prefix = "/bench", version = "v1")]
struct BenchController;

impl BenchController {
    #[get("/work")]
    async fn work() -> &'static str {
        "ok"
    }
}

impl_routes!(BenchController, state BenchState => [
    GET "/work" with () => BenchController::work,
]);

#[module(
    controllers = [BenchController],
    providers = [BenchState],
)]
struct BenchModule;

fn bench_router_middleware_stack(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let router = NestFactory::create::<BenchModule>()
        .use_request_id()
        .use_request_context()
        .use_request_tracing(RequestTracingOptions::builder())
        .use_compression()
        .use_concurrency_limit(128)
        .into_router();

    c.bench_function("router_get_v1_bench_work_middleware_stack", |b| {
        b.to_async(&rt).iter(|| async {
            let _res = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/v1/bench/work")
                        .method("GET")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
        })
    });
}

criterion_group!(benches, bench_router_middleware_stack);
criterion_main!(benches);
