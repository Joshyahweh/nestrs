#![cfg(feature = "openapi")]

#[cfg(feature = "test-hooks")]
fn reset_global_registries() {
    nestrs::core::RouteRegistry::clear_for_tests();
    nestrs::core::MetadataRegistry::clear_for_tests();
}

use axum::body::{to_bytes, Body};
use axum::http::Request;
use nestrs::prelude::*;
use nestrs_openapi::OpenApiOptions;
use serde_json::json;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/o", version = "v1")]
struct OpenApiController;

#[routes(state = AppState)]
impl OpenApiController {
    #[openapi(
        summary = "Liveness probe",
        tag = "health",
        responses = ((200, "Plain text pong"))
    )]
    #[get("/ping")]
    async fn ping() -> &'static str {
        "pong"
    }
}

#[module(controllers = [OpenApiController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn openapi_json_includes_registered_routes() {
    #[cfg(feature = "test-hooks")]
    reset_global_registries();

    let router = NestFactory::create::<AppModule>()
        .enable_openapi()
        .into_router();

    let res = router
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    let bytes = to_bytes(res.into_body(), 1024 * 1024).await.expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

    assert!(
        doc["paths"].get("/v1/o/ping").is_some(),
        "expected /v1/o/ping to be present in OpenAPI doc"
    );

    let op = &doc["paths"]["/v1/o/ping"]["get"];
    assert_eq!(op["tags"][0], "health", "tag from #[openapi]");
    assert_eq!(op["summary"], "Liveness probe");
    assert_eq!(op["responses"]["200"]["description"], "Plain text pong");
    let oid = op["operationId"].as_str().expect("operationId");
    assert!(oid.contains("ping"), "unexpected operationId: {oid}");
}

#[controller(prefix = "/sec", version = "v1")]
struct OpenApiSecController;

#[routes(state = AppState)]
impl OpenApiSecController {
    #[get("/public")]
    async fn public_ok() -> &'static str {
        "ok"
    }

    #[roles("admin")]
    #[get("/admin")]
    async fn admin_ok() -> &'static str {
        "ok"
    }
}

#[module(controllers = [OpenApiSecController], providers = [AppState])]
struct SecAppModule;

#[tokio::test]
async fn openapi_infers_operation_security_when_roles_metadata_present() {
    #[cfg(feature = "test-hooks")]
    reset_global_registries();

    let router = NestFactory::create::<SecAppModule>()
        .enable_openapi_with_options(OpenApiOptions {
            infer_route_security_from_roles: true,
            roles_security_scheme: "bearerAuth".into(),
            components: Some(json!({
                "securitySchemes": {
                    "bearerAuth": {
                        "type": "http",
                        "scheme": "bearer",
                        "bearerFormat": "JWT"
                    }
                }
            })),
            ..Default::default()
        })
        .into_router();

    let res = router
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    let bytes = to_bytes(res.into_body(), 1024 * 1024).await.expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

    let public_path = "/v1/sec/public";
    let admin_path = "/v1/sec/admin";
    assert!(
        doc["paths"].get(public_path).is_some(),
        "expected {public_path}, paths={:?}",
        doc["paths"]
    );

    let pub_op = &doc["paths"][public_path]["get"];
    assert!(
        pub_op.get("security").is_none(),
        "public route should not have operation security: {pub_op:?}"
    );

    let admin_op = &doc["paths"][admin_path]["get"];
    let sec = admin_op["security"]
        .as_array()
        .expect("admin security array");
    assert_eq!(sec.len(), 1);
    assert!(sec[0].get("bearerAuth").is_some());

    assert!(
        doc["components"]["securitySchemes"]["bearerAuth"].is_object(),
        "components.securitySchemes should be present"
    );
}
