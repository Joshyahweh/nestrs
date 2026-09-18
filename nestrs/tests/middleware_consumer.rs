mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::RegistryResetGuard;
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/admin")]
struct AdminController;

#[routes(state = AppState)]
impl AdminController {
    #[get("/")]
    async fn root() -> &'static str {
        "admin"
    }

    #[get("/health")]
    async fn health() -> &'static str {
        "ok"
    }
}

#[controller(prefix = "/public")]
struct PublicController;

#[routes(state = AppState)]
impl PublicController {
    #[get("/")]
    async fn root() -> &'static str {
        "public"
    }
}

#[module(
    controllers = [AdminController, PublicController],
    providers = [AppState],
)]
struct AppModule;

#[tokio::test]
async fn middleware_consumer_applies_only_to_included_routes() {
    let _guard = RegistryResetGuard::new();
    let router = NestFactory::create::<AppModule>()
        .configure_middleware(
            MiddlewareConsumer::new()
                .apply_fn(|mut req, next| async move {
                    req.headers_mut()
                        .insert("x-mw", axum::http::HeaderValue::from_static("yes"));
                    let mut res = next.run(req).await;
                    res.headers_mut()
                        .insert("x-mw-applied", axum::http::HeaderValue::from_static("1"));
                    res
                })
                .for_routes(["/admin"])
                .exclude(["/admin/health"]),
        )
        .into_router();

    let admin = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin.status(), StatusCode::OK);
    assert_eq!(
        admin.headers().get("x-mw-applied").map(|v| v.as_bytes()),
        Some(&b"1"[..])
    );

    let health = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    assert!(health.headers().get("x-mw-applied").is_none());

    let public = router
        .oneshot(
            Request::builder()
                .uri("/public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(public.status(), StatusCode::OK);
    assert!(public.headers().get("x-mw-applied").is_none());
}
