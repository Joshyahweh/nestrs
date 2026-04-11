#![cfg(feature = "openapi")]

use axum::body::{to_bytes, Body};
use axum::http::Request;
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/o", version = "v1")]
struct OpenApiController;

#[routes(state = AppState)]
impl OpenApiController {
    #[get("/ping")]
    async fn ping() -> &'static str {
        "pong"
    }
}

#[module(controllers = [OpenApiController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn openapi_json_includes_registered_routes() {
    let router = NestFactory::create::<AppModule>()
        .enable_openapi()
        .into_router();

    let res = router
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    let bytes = to_bytes(res.into_body(), 1024 * 1024).await.expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

    assert!(
        doc["paths"].get("/v1/o/ping").is_some(),
        "expected /v1/o/ping to be present in OpenAPI doc"
    );
}
