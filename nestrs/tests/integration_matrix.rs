//! Combines **openapi**, **CSRF**, **URI-style version segment** (`#[controller(version)]`), **guards**,
//! and **backpressure-style** `NestFactory` options in one process to catch macro/DI ordering bugs.
//!
//! Built only with `--features openapi,csrf,test-hooks` (see `Cargo.toml` `[[test]]`).

#![cfg(all(
    feature = "openapi",
    feature = "csrf",
    feature = "test-hooks",
    feature = "cookies"
))]

use axum::body::{to_bytes, Body};
use axum::http::request::Parts;
use axum::http::{header, Request as HttpRequest, StatusCode};
use nestrs::prelude::*;
use std::time::Duration;
use tower::util::ServiceExt;
use tower::Service;

fn reset_global_registries() {
    nestrs::core::RouteRegistry::clear_for_tests();
    nestrs::core::MetadataRegistry::clear_for_tests();
}

#[derive(Default)]
#[injectable]
struct AppState;

#[derive(Default)]
struct MatrixHeaderGuard;

#[async_trait]
impl CanActivate for MatrixHeaderGuard {
    async fn can_activate(&self, parts: &Parts) -> Result<(), GuardError> {
        if parts.headers.get("x-matrix-probe").is_some() {
            Ok(())
        } else {
            Err(GuardError::forbidden("missing x-matrix-probe"))
        }
    }
}

#[controller(prefix = "/mx", version = "v2")]
struct MatrixController;

#[routes(state = AppState)]
impl MatrixController {
    #[openapi(
        summary = "Matrix status",
        tag = "matrix",
        responses = ((200, "ok"))
    )]
    #[get("/status")]
    async fn status() -> &'static str {
        "up"
    }

    #[post("/mutate")]
    #[use_guards(MatrixHeaderGuard)]
    async fn mutate() -> &'static str {
        "mutated"
    }
}

#[module(controllers = [MatrixController], providers = [AppState])]
struct AppModule;

fn matrix_router() -> axum::Router {
    NestFactory::create::<AppModule>()
        .set_global_prefix("gw")
        .enable_openapi()
        .use_cookies()
        .use_csrf_protection(CsrfProtectionConfig::default())
        .use_request_timeout(Duration::from_secs(30))
        .use_concurrency_limit(64)
        .use_body_limit(16 * 1024)
        .into_router()
}

#[tokio::test]
async fn matrix_openapi_lists_versioned_route_under_prefix() {
    reset_global_registries();

    let router = matrix_router();

    let res = router
        .oneshot(
            HttpRequest::builder()
                .uri("/openapi.json")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = to_bytes(res.into_body(), 1024 * 1024).await.expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

    let paths = doc["paths"].as_object().expect("paths object");
    let key = paths
        .keys()
        .find(|k| k.contains("v2") && k.contains("mx") && k.contains("status"))
        .unwrap_or_else(|| panic!("expected a path key containing v2/mx/status, got {paths:?}"));

    let op = &doc["paths"][key]["get"];
    assert_eq!(op["summary"], "Matrix status");
    assert_eq!(op["tags"][0], "matrix");
}

#[tokio::test]
async fn matrix_post_requires_csrf_and_guard_header() {
    reset_global_registries();

    let mut router = matrix_router();
    ServiceExt::<HttpRequest<Body>>::ready(&mut router)
        .await
        .expect("router ready");

    let forbidden = router
        .call(
            HttpRequest::builder()
                .uri("/gw/v2/mx/mutate")
                .method("POST")
                .header(header::COOKIE, "csrf_token=secret")
                .header("x-csrf-token", "secret")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");
    assert_eq!(
        forbidden.status(),
        StatusCode::FORBIDDEN,
        "guard should reject without x-matrix-probe"
    );

    let ok = router
        .call(
            HttpRequest::builder()
                .uri("/gw/v2/mx/mutate")
                .method("POST")
                .header(header::COOKIE, "csrf_token=secret")
                .header("x-csrf-token", "secret")
                .header("x-matrix-probe", "1")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");
    assert_eq!(ok.status(), StatusCode::OK);
    let body = to_bytes(ok.into_body(), 1024).await.expect("body");
    assert_eq!(body.as_ref(), b"mutated");
}
