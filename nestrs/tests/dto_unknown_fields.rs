use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;
use validator::Validate;

#[derive(Default)]
#[injectable]
struct AppState;

#[dto]
struct StrictDto {
    #[IsString]
    value: String,
}

#[dto]
struct NestedStrictInnerDto {
    #[IsString]
    label: String,
}

#[dto]
struct NestedStrictDto {
    #[ValidateNested]
    inner: NestedStrictInnerDto,
    #[IsOptional]
    #[ValidateNested]
    optional_inner: Option<NestedStrictInnerDto>,
}

#[dto(allow_unknown_fields)]
struct LooseDto {
    #[IsString]
    value: String,
}

#[controller(prefix = "/dto", version = "v1")]
struct DtoController;

#[routes(state = AppState)]
impl DtoController {
    #[post("/strict")]
    async fn strict(ValidatedBody(dto): ValidatedBody<StrictDto>) -> String {
        dto.value
    }

    #[post("/nested")]
    async fn nested(ValidatedBody(dto): ValidatedBody<NestedStrictDto>) -> String {
        dto.inner.label
    }

    #[post("/loose")]
    async fn loose(ValidatedBody(dto): ValidatedBody<LooseDto>) -> String {
        dto.value
    }
}

#[module(controllers = [DtoController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn dto_rejects_unknown_top_level_fields_by_default() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/dto/strict")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"value":"ok","extra":"boom"}"#))
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn dto_rejects_unknown_nested_fields() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/dto/nested")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"inner":{"label":"ok","extra":"boom"},"optional_inner":null}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn dto_allows_optional_nested_fields_when_omitted() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/dto/nested")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"inner":{"label":"ok"}}"#))
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn dto_allow_unknown_fields_option_allows_extra_keys() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/dto/loose")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"value":"ok","extra":"allowed"}"#))
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::OK);
}
