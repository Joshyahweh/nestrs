use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/m", version = "v1")]
struct MetaController;

#[routes(state = AppState)]
impl MetaController {
    #[get("/admin")]
    #[roles("admin")]
    #[use_guards(XRoleMetadataGuard)]
    async fn admin() -> &'static str {
        "admin"
    }

    #[get("/user")]
    #[roles("user")]
    #[use_guards(XRoleMetadataGuard)]
    async fn user() -> &'static str {
        "user"
    }
}

#[module(controllers = [MetaController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn roles_metadata_is_visible_to_guards() {
    let router = NestFactory::create::<AppModule>().into_router();

    let ok = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/m/admin")
                .method("GET")
                .header("x-role", "admin")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    assert_eq!(ok.status(), StatusCode::OK);

    let denied = router
        .oneshot(
            Request::builder()
                .uri("/v1/m/admin")
                .method("GET")
                .header("x-role", "user")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
}
