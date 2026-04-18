//! Deterministic ordering for guards, interceptors, and route exception filters.
//!
//! Contract is documented in the mdBook: `docs/src/http-pipeline-order.md` and `impl_routes!` rustdoc.

mod common;

use std::sync::Mutex;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use serial_test::serial;
use tower::util::ServiceExt;

use crate::common::RegistryResetGuard;

static GUARD_ORDER: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static INTERCEPTOR_ORDER: Mutex<Vec<u32>> = Mutex::new(Vec::new());

#[derive(Default)]
#[injectable]
struct AppState;

#[derive(Default)]
struct MarkGuard1;

#[async_trait]
impl CanActivate for MarkGuard1 {
    async fn can_activate(&self, _parts: &axum::http::request::Parts) -> Result<(), GuardError> {
        GUARD_ORDER.lock().expect("poisoned").push(1);
        Ok(())
    }
}

#[derive(Default)]
struct MarkGuard2;

#[async_trait]
impl CanActivate for MarkGuard2 {
    async fn can_activate(&self, _parts: &axum::http::request::Parts) -> Result<(), GuardError> {
        GUARD_ORDER.lock().expect("poisoned").push(2);
        Ok(())
    }
}

#[derive(Default)]
struct TraceInterceptor1;

#[async_trait]
impl Interceptor for TraceInterceptor1 {
    async fn intercept(
        &self,
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        INTERCEPTOR_ORDER.lock().expect("poisoned").push(1);
        next.run(req).await
    }
}

#[derive(Default)]
struct TraceInterceptor2;

#[async_trait]
impl Interceptor for TraceInterceptor2 {
    async fn intercept(
        &self,
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        INTERCEPTOR_ORDER.lock().expect("poisoned").push(2);
        next.run(req).await
    }
}

#[derive(Default)]
struct SuffixFilter1;

#[async_trait]
impl ExceptionFilter for SuffixFilter1 {
    async fn catch(&self, mut ex: HttpException) -> axum::response::Response {
        ex.message = format!("A:{}", ex.message);
        ex.into_response()
    }
}

#[derive(Default)]
struct SuffixFilter2;

#[async_trait]
impl ExceptionFilter for SuffixFilter2 {
    async fn catch(&self, mut ex: HttpException) -> axum::response::Response {
        ex.message = format!("B:{}", ex.message);
        ex.into_response()
    }
}

#[controller(prefix = "/ord", version = "v1")]
struct OrderController;

#[routes(state = AppState)]
impl OrderController {
    #[get("/guards")]
    #[use_guards(MarkGuard1, MarkGuard2)]
    async fn guards_probe() -> String {
        let g = GUARD_ORDER.lock().expect("poisoned").clone();
        format!("guards={g:?}")
    }

    #[get("/interceptors")]
    #[use_interceptors(TraceInterceptor1, TraceInterceptor2)]
    async fn interceptors_probe() -> &'static str {
        "ok"
    }

    #[get("/filters")]
    #[use_filters(SuffixFilter1, SuffixFilter2)]
    async fn filters_probe() -> Result<&'static str, HttpException> {
        Err(NotFoundException::new("x"))
    }
}

#[module(controllers = [OrderController], providers = [AppState])]
struct AppModule;

fn clear_traces() {
    GUARD_ORDER.lock().expect("poisoned").clear();
    INTERCEPTOR_ORDER.lock().expect("poisoned").clear();
}

#[tokio::test]
#[serial]
async fn route_guards_run_left_to_right() {
    let _registry_guard = RegistryResetGuard::new();
    clear_traces();
    let router = NestFactory::create::<AppModule>().into_router();

    let res = router
        .oneshot(
            Request::builder()
                .uri("/v1/ord/guards")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.expect("read");
    assert_eq!(String::from_utf8_lossy(&body), "guards=[1, 2]");
}

#[tokio::test]
#[serial]
async fn route_interceptors_outer_is_first_in_attribute_list() {
    let _registry_guard = RegistryResetGuard::new();
    clear_traces();
    let router = NestFactory::create::<AppModule>().into_router();

    let res = router
        .oneshot(
            Request::builder()
                .uri("/v1/ord/interceptors")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(res.status(), StatusCode::OK);
    let order = INTERCEPTOR_ORDER.lock().expect("poisoned").clone();
    assert_eq!(order, vec![1, 2], "outer interceptor should run first");
}

#[tokio::test]
#[serial]
async fn route_filters_inner_listed_last_transforms_first() {
    let _registry_guard = RegistryResetGuard::new();
    let router = NestFactory::create::<AppModule>().into_router();

    let res = router
        .oneshot(
            Request::builder()
                .uri("/v1/ord/filters")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(res.into_body(), 4096).await.expect("read");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        v["message"].as_str().expect("message"),
        "A:B:x",
        "SuffixFilter2 (inner) runs before SuffixFilter1 (outer)"
    );
}
