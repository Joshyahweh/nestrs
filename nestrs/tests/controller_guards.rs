//! Controller-level `controller_guards(...)` run **outside** per-route guards (NestJS order).

use axum::body::{to_bytes, Body};
use axum::http::request::Parts;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[derive(Default)]
struct CtrlDenyGuard;

#[async_trait]
impl CanActivate for CtrlDenyGuard {
    async fn can_activate(&self, _parts: &Parts) -> Result<(), GuardError> {
        Err(GuardError::forbidden("ctrl-deny"))
    }
}

#[derive(Default)]
struct RouteDenyGuard;

#[async_trait]
impl CanActivate for RouteDenyGuard {
    async fn can_activate(&self, _parts: &Parts) -> Result<(), GuardError> {
        Err(GuardError::forbidden("route-deny"))
    }
}

#[controller(prefix = "/c", version = "v1")]
struct CtrlDenyController;

impl CtrlDenyController {
    #[get("/")]
    async fn root() -> &'static str {
        "should-not-run"
    }
}

impl_routes!(CtrlDenyController, state AppState, controller_guards(CtrlDenyGuard) => [
    GET "/" with () => CtrlDenyController::root,
]);

#[controller(prefix = "/c", version = "v1")]
struct OrderProbeController;

impl OrderProbeController {
    #[get("/order")]
    async fn order() -> &'static str {
        "should-not-run"
    }
}

impl_routes!(OrderProbeController, state AppState, controller_guards(CtrlDenyGuard) => [
    GET "/order" with (RouteDenyGuard) => OrderProbeController::order,
]);

#[module(controllers = [CtrlDenyController], providers = [AppState])]
struct CtrlDenyModule;

#[module(controllers = [OrderProbeController], providers = [AppState])]
struct OrderProbeModule;

#[tokio::test]
async fn controller_guard_blocks_all_routes() {
    let router = NestFactory::create::<CtrlDenyModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/c")
                .method("GET")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = to_bytes(response.into_body(), 4096).await.expect("body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["message"], "ctrl-deny");
}

#[tokio::test]
async fn controller_guards_are_outer_relative_to_route_guards() {
    let router = NestFactory::create::<OrderProbeModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/c/order")
                .method("GET")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = to_bytes(response.into_body(), 4096).await.expect("body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(
        v["message"], "ctrl-deny",
        "when both deny, outer (controller) guard must run first"
    );
}
