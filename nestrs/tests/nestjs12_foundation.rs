//! Wave 0 — NestJS 12 foundation: errorCode, route conflicts, schema pipe.
#![cfg(feature = "test-hooks")]

use axum::response::IntoResponse;
use nestrs::prelude::*;
use nestrs_core::RouteRegistry;
use serde_json::Value;

#[tokio::test]
async fn http_exception_error_code_in_json_body() {
    use axum::body::to_bytes;
    let ex = BadRequestException::new("Password is too weak").with_error_code("WEAK_PASSWORD");
    let res = ex.into_response();
    let bytes = to_bytes(res.into_body(), 1024).await.expect("body");
    let v: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["statusCode"], 400);
    assert_eq!(v["errorCode"], "WEAK_PASSWORD");
    assert_eq!(v["message"], "Password is too weak");
}

#[test]
fn route_conflict_detects_duplicates() {
    RouteRegistry::clear_for_tests();
    RouteRegistry::register("GET", "/users/:id", "a::one");
    RouteRegistry::register("GET", "/users/:id", "a::two");
    let report = RouteRegistry::conflict_report();
    assert_eq!(report.duplicates.len(), 1);
    assert_eq!(report.duplicates[0].first_handler, "a::one");
    assert_eq!(report.duplicates[0].second_handler, "a::two");
}

#[test]
fn route_conflict_detects_shadow() {
    RouteRegistry::clear_for_tests();
    RouteRegistry::register("GET", "/users/:id", "a::param");
    RouteRegistry::register("GET", "/users/me", "a::me");
    let report = RouteRegistry::conflict_report();
    assert!(
        !report.shadows.is_empty(),
        "expected shadow of /users/me by /users/:id"
    );
}

#[tokio::test]
async fn schema_validation_pipe_rejects_invalid() {
    use nestrs::core::PipeTransform;
    use nestrs::SchemaValidationPipe;
    use validator::Validate;

    #[derive(Debug, Validate, schemars::JsonSchema)]
    struct Dto {
        #[validate(length(min = 3))]
        name: String,
    }

    let dto = Dto { name: "a".into() };
    let err = SchemaValidationPipe
        .transform(dto)
        .await
        .expect_err("invalid");
    assert_eq!(err.status, axum::http::StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn execution_context_exposes_handler_meta() {
    use nestrs_core::ExecutionContext;
    use std::any::TypeId;

    let parts = axum::http::Request::builder()
        .method("GET")
        .uri("/api/users")
        .body(())
        .unwrap()
        .into_parts()
        .0;
    let ctx = ExecutionContext::from_http_parts(&parts).with_handler_meta(
        Some("app::UsersController"),
        Some(TypeId::of::<u8>()),
        Some("app::UsersController::list"),
        Some(TypeId::of::<u16>()),
    );
    assert_eq!(ctx.get_class_name(), Some("app::UsersController"));
    assert_eq!(ctx.get_handler_name(), Some("app::UsersController::list"));
    assert!(ctx.get_class().is_some());
    assert!(ctx.get_handler().is_some());
}
