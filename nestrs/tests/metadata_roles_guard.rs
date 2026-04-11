use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[derive(Default)]
struct RolesGuard;

#[async_trait]
impl CanActivate for RolesGuard {
    async fn can_activate(&self, parts: &axum::http::request::Parts) -> Result<(), GuardError> {
        let handler = parts
            .extensions
            .get::<nestrs::core::HandlerKey>()
            .map(|h| h.0)
            .ok_or_else(|| GuardError::forbidden("missing handler key"))?;

        let allowed = MetadataRegistry::get(handler, "roles")
            .ok_or_else(|| GuardError::forbidden("missing roles metadata"))?;

        let role = parts
            .headers
            .get("x-role")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let is_allowed = allowed.split(',').any(|r| r.trim() == role);
        if is_allowed {
            Ok(())
        } else {
            Err(GuardError::forbidden("forbidden"))
        }
    }
}

#[controller(prefix = "/m", version = "v1")]
struct MetaController;

#[routes(state = AppState)]
impl MetaController {
    #[get("/admin")]
    #[roles("admin")]
    #[use_guards(RolesGuard)]
    async fn admin() -> &'static str {
        "admin"
    }

    #[get("/user")]
    #[roles("user")]
    #[use_guards(RolesGuard)]
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

