use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/root", version = "v1")]
struct RootController;

impl RootController {
    #[get("/")]
    async fn root() -> &'static str {
        "root"
    }
}

impl_routes!(RootController, state AppState => [
    GET "/" with () => RootController::root,
]);

#[module(
    controllers = [RootController],
    providers = [AppState],
)]
struct RootModule;

#[derive(Default)]
#[injectable]
struct FeatureState;

#[controller(prefix = "/feature", version = "v1")]
struct FeatureController;

impl FeatureController {
    #[get("/")]
    async fn feature() -> &'static str {
        "feature"
    }
}

impl_routes!(FeatureController, state FeatureState => [
    GET "/" with () => FeatureController::feature,
]);

#[module(
    controllers = [FeatureController],
    providers = [FeatureState],
)]
struct FeatureModule;

#[tokio::test]
async fn create_with_modules_merges_dynamic_feature_router() {
    let router = NestFactory::create_with_modules::<RootModule, _>([
        DynamicModule::from_module::<FeatureModule>(),
    ])
    .into_router();

    let root = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/root")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(root.status(), StatusCode::OK);

    let feature = router
        .oneshot(
            Request::builder()
                .uri("/v1/feature")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(feature.status(), StatusCode::OK);
}

