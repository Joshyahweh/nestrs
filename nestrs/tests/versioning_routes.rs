use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/api/", version = "/v1/")]
struct V1Controller;

impl V1Controller {
    #[get("/")]
    async fn root() -> &'static str {
        "v1"
    }

    #[get("/feature")]
    async fn feature() -> &'static str {
        "feature-v2"
    }
}

impl_routes!(V1Controller, state AppState => [
    GET "/" with () => V1Controller::root,
    @ver("/v2/") GET "/feature" with () => V1Controller::feature,
]);

#[version("v2")]
#[controller(prefix = "/api/")]
struct V2Controller;

impl V2Controller {
    #[get("/")]
    async fn root() -> &'static str {
        "v2"
    }
}

impl_routes!(V2Controller, state AppState => [
    GET "/" with () => V2Controller::root,
]);

#[module(
    controllers = [V1Controller, V2Controller],
    providers = [AppState],
)]
struct AppModule;

#[tokio::test]
async fn route_level_version_override_has_precedence() {
    let (_, router) = <AppModule as Module>::build();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v2/api/feature")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn controller_versions_are_isolated() {
    let (_, router) = <AppModule as Module>::build();

    let v1 = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(v1.status(), StatusCode::OK);

    let v2 = router
        .oneshot(
            Request::builder()
                .uri("/v2/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(v2.status(), StatusCode::OK);
}

#[tokio::test]
async fn unversioned_route_is_not_exposed() {
    let (_, router) = <AppModule as Module>::build();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
