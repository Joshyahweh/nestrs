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

fn bench_router_hot_path(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let router = NestFactory::create::<BenchModule>()
        .use_concurrency_limit(128)
        .into_router();

    c.bench_function("router_get_v1_bench_ping", |b| {
        b.to_async(&rt).iter(|| async {
            let _res = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/v1/bench/ping")
                        .method("GET")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
        })
    });
}

criterion_group!(benches, bench_router_hot_path);
criterion_main!(benches);
