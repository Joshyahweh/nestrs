use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/t", version = "v1")]
struct TestController;

#[routes(state = AppState)]
impl TestController {
    #[get("/")]
    async fn root() -> &'static str {
        "ok"
    }

    #[get("/feature")]
    #[ver("v2")]
    async fn feature() -> &'static str {
        "v2"
    }
}

#[module(controllers = [TestController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn routes_macro_registers_handlers_and_ver_overrides() {
    let router = NestFactory::create::<AppModule>().into_router();

    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/t")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);

    let res = router
        .oneshot(
            Request::builder()
                .uri("/v2/t/feature")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
}
