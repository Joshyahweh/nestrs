mod common;

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use common::RegistryResetGuard;
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/items", version = "v1")]
struct ItemsV1Controller;

#[routes(state = AppState)]
impl ItemsV1Controller {
    #[get("/")]
    async fn root() -> &'static str {
        "v1"
    }
}

#[controller(prefix = "/items", version = "v2")]
struct ItemsV2Controller;

#[routes(state = AppState)]
impl ItemsV2Controller {
    #[get("/")]
    async fn root() -> &'static str {
        "v2"
    }
}

#[module(
    controllers = [ItemsV1Controller, ItemsV2Controller],
    providers = [AppState],
)]
struct AppModule;

async fn response_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 1024)
        .await
        .expect("body should be readable");
    String::from_utf8(bytes.to_vec()).expect("body should be utf8")
}

#[tokio::test]
async fn header_versioning_routes_the_unversioned_path() {
    let _registry_guard = RegistryResetGuard::new();
    let router = NestFactory::create::<AppModule>()
        .enable_header_versioning("x-api-version", None)
        .into_router();

    let v1 = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/items")
                .method("GET")
                .header("x-api-version", "v1")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(v1.status(), StatusCode::OK);
    assert_eq!(response_text(v1).await, "v1");

    let v2 = router
        .oneshot(
            Request::builder()
                .uri("/items")
                .method("GET")
                .header("x-api-version", "v2")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(v2.status(), StatusCode::OK);
    assert_eq!(response_text(v2).await, "v2");
}

#[tokio::test]
async fn media_type_versioning_routes_the_unversioned_path() {
    let _registry_guard = RegistryResetGuard::new();
    let router = NestFactory::create::<AppModule>()
        .enable_media_type_versioning(None)
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/items")
                .method("GET")
                .header(header::ACCEPT, "application/vnd.api+json;version=v2")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_text(response).await, "v2");
}

#[tokio::test]
async fn header_versioning_inserts_version_after_global_prefix() {
    let _registry_guard = RegistryResetGuard::new();
    let router = NestFactory::create::<AppModule>()
        .set_global_prefix("api")
        .enable_header_versioning("x-api-version", None)
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/items")
                .method("GET")
                .header("x-api-version", "v2")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_text(response).await, "v2");
}
