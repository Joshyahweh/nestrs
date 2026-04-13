#![cfg(all(feature = "csrf", feature = "cookies"))]

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/api")]
struct ApiController;

#[routes(state = AppState)]
impl ApiController {
    #[post("/echo")]
    async fn echo() -> &'static str {
        "ok"
    }
}

#[module(controllers = [ApiController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn csrf_rejects_post_without_matching_header() {
    let router = NestFactory::create::<AppModule>()
        .use_cookies()
        .use_csrf_protection(nestrs::CsrfProtectionConfig::default())
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/echo")
                .method("POST")
                .header(header::COOKIE, "csrf_token=abc")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn csrf_allows_post_when_cookie_matches_header() {
    let router = NestFactory::create::<AppModule>()
        .use_cookies()
        .use_csrf_protection(nestrs::CsrfProtectionConfig::default())
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/echo")
                .method("POST")
                .header(header::COOKIE, "csrf_token=secret")
                .header("x-csrf-token", "secret")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::OK);
}
