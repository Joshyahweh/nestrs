use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AState;

#[controller(prefix = "/a", version = "v1")]
struct AController;

impl AController {
    #[get("/")]
    async fn ok() -> &'static str {
        "a"
    }
}

impl_routes!(AController, state AState => [
    GET "/" with () => AController::ok,
]);

#[derive(Default)]
#[injectable]
struct BState;

#[controller(prefix = "/b", version = "v1")]
struct BController;

impl BController {
    #[get("/")]
    async fn ok() -> &'static str {
        "b"
    }
}

impl_routes!(BController, state BState => [
    GET "/" with () => BController::ok,
]);

#[module(
    imports = [BModule],
    controllers = [AController],
    providers = [AState],
)]
struct AModule;

#[module(
    imports = [AModule],
    controllers = [BController],
    providers = [BState],
)]
struct BModule;

#[test]
fn cyclic_module_imports_panic_with_clear_message() {
    let err = catch_unwind(AssertUnwindSafe(|| {
        let _ = <AModule as Module>::build();
    }))
    .expect_err("should panic on cyclic module imports");

    let msg = err
        .downcast_ref::<String>()
        .map(|s| s.as_str())
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("<non-string panic>");

    assert!(
        msg.contains("Circular module dependency detected"),
        "unexpected panic message: {msg}"
    );
}

#[module(
    imports = [BForwardModule],
    controllers = [AController],
    providers = [AState],
)]
struct AForwardModule;

#[module(
    imports = [forward_ref::<AForwardModule>()],
    controllers = [BController],
    providers = [BState],
)]
struct BForwardModule;

#[tokio::test]
async fn forward_ref_breaks_back_edge_and_builds_router() {
    let (_, router) = <AForwardModule as Module>::build();

    let a = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/a")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(a.status(), StatusCode::OK);

    let b = router
        .oneshot(
            Request::builder()
                .uri("/v1/b")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(b.status(), StatusCode::OK);
}
