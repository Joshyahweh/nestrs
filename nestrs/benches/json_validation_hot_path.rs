//! Micro-benchmark: full HTTP hot path with JSON body + `validator` on a `#[dto]` type.

use axum::body::Body;
use axum::http::Request;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nestrs::prelude::*;
use nestrs::ValidatedBody;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct BenchState;

#[dto]
struct BenchSignupBody {
    #[IsEmail]
    email: String,
}

#[controller(prefix = "/bench", version = "v1")]
struct JsonBenchController;

impl JsonBenchController {
    #[post("/signup")]
    async fn signup(ValidatedBody(body): ValidatedBody<BenchSignupBody>) -> &'static str {
        black_box(body.email.len());
        "ok"
    }
}

impl_routes!(JsonBenchController, state BenchState => [
    POST "/signup" with () => JsonBenchController::signup,
]);

#[module(
    controllers = [JsonBenchController],
    providers = [BenchState],
)]
struct JsonBenchModule;

const VALID_JSON: &[u8] = br#"{"email":"user@example.com"}"#;

fn bench_router_post_json_validated_body(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let router = NestFactory::create::<JsonBenchModule>().into_router();

    c.bench_function("router_post_v1_bench_signup_validated_json", |b| {
        b.to_async(&rt).iter(|| async {
            let _res = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/bench/signup")
                        .header("content-type", "application/json")
                        .body(Body::from(VALID_JSON))
                        .expect("request"),
                )
                .await
                .expect("response");
        })
    });
}

criterion_group!(benches, bench_router_post_json_validated_body);
criterion_main!(benches);
