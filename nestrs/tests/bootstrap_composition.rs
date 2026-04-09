use axum::body::{to_bytes, Body};
use axum::http::request::Parts;
use axum::http::{header, HeaderName, HeaderValue, Request, StatusCode};
use flate2::write::GzEncoder;
use flate2::Compression;
use nestrs::prelude::*;
use nestrs::runtime_is_production;
use serial_test::serial;
use std::io::Write;
use tower::util::ServiceExt;
use tower_http::set_header::SetResponseHeaderLayer;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

/// Clears `NESTRS_ENV`, `APP_ENV`, and `RUST_ENV` for the scope of a test, then restores previous values.
struct ClearProductionEnvGuard {
    nestrs: Option<String>,
    app: Option<String>,
    rust: Option<String>,
}

impl ClearProductionEnvGuard {
    fn new() -> Self {
        let nestrs = std::env::var("NESTRS_ENV").ok();
        let app = std::env::var("APP_ENV").ok();
        let rust = std::env::var("RUST_ENV").ok();
        std::env::remove_var("NESTRS_ENV");
        std::env::remove_var("APP_ENV");
        std::env::remove_var("RUST_ENV");
        Self { nestrs, app, rust }
    }
}

impl Drop for ClearProductionEnvGuard {
    fn drop(&mut self) {
        restore_env_var("NESTRS_ENV", self.nestrs.take());
        restore_env_var("APP_ENV", self.app.take());
        restore_env_var("RUST_ENV", self.rust.take());
    }
}

fn restore_env_var(key: &str, value: Option<String>) {
    match value {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

#[derive(Default)]
#[injectable]
struct AppState;

#[dto]
struct SignupDto {
    #[IsEmail]
    email: String,
    #[IsString]
    #[Length(min = 3, max = 32)]
    username: String,
}

#[controller(prefix = "/api", version = "v1")]
struct AppController;

impl AppController {
    #[get("/")]
    async fn root() -> &'static str {
        "ok"
    }

    #[post("/echo")]
    async fn echo(body: axum::body::Bytes) -> String {
        String::from_utf8_lossy(&body).to_string()
    }

    #[post("/validate")]
    async fn validate(ValidatedBody(dto): ValidatedBody<SignupDto>) -> String {
        dto.username
    }

    #[get("/slow")]
    async fn slow() -> &'static str {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        "slow"
    }

    #[get("/internal-error")]
    async fn internal_error() -> Result<&'static str, HttpException> {
        Err(InternalServerErrorException::new("secret-internal-detail"))
    }

    #[get("/guarded-ok")]
    async fn guarded_ok() -> &'static str {
        "guarded-ok"
    }

    #[get("/guarded-deny")]
    async fn guarded_deny() -> &'static str {
        "unreachable"
    }

    #[get("/ctx")]
    async fn ctx_preview(ctx: RequestContext) -> String {
        format!(
            "{}|{}|{}",
            ctx.method,
            ctx.path_and_query,
            ctx.request_id.as_deref().unwrap_or("")
        )
    }

    /// Body length > 32 so [`tower_http::compression::CompressionLayer`] default predicate applies.
    #[get("/compressible")]
    async fn compressible() -> String {
        "y".repeat(64)
    }
}

#[derive(Default)]
struct AllowForTests;

#[async_trait]
impl CanActivate for AllowForTests {
    async fn can_activate(&self, _parts: &Parts) -> Result<(), GuardError> {
        Ok(())
    }
}

#[derive(Default)]
struct DenyForTests;

#[async_trait]
impl CanActivate for DenyForTests {
    async fn can_activate(&self, _parts: &Parts) -> Result<(), GuardError> {
        Err(GuardError::forbidden("denied by test guard"))
    }
}

impl_routes!(AppController, state AppState => [
    GET "/" with () => AppController::root,
    GET "/slow" with () => AppController::slow,
    POST "/echo" with () => AppController::echo,
    POST "/validate" with () => AppController::validate,
    GET "/internal-error" with () => AppController::internal_error,
    GET "/guarded-ok" with (AllowForTests) => AppController::guarded_ok,
    GET "/guarded-deny" with (DenyForTests) => AppController::guarded_deny,
    GET "/ctx" with () => AppController::ctx_preview,
    GET "/compressible" with () => AppController::compressible,
]);

#[tokio::test]
async fn validated_body_returns_422_with_validation_errors() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/validate")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"email":"not-an-email","username":"ab"}"#))
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = response.into_body();
    let bytes = to_bytes(body, 64 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["statusCode"], 422);
    assert_eq!(v["message"], "Validation failed");
    assert!(v["errors"].is_array());
}

#[module(
    controllers = [AppController],
    providers = [AppState],
)]
struct AppModule;

struct ReadinessAlwaysUp;

#[async_trait]
impl HealthIndicator for ReadinessAlwaysUp {
    fn name(&self) -> &'static str {
        "always_up"
    }

    async fn check(&self) -> HealthStatus {
        HealthStatus::Up
    }
}

struct ReadinessAlwaysDown;

#[async_trait]
impl HealthIndicator for ReadinessAlwaysDown {
    fn name(&self) -> &'static str {
        "always_down"
    }

    async fn check(&self) -> HealthStatus {
        HealthStatus::down("test failure")
    }
}

#[tokio::test]
async fn use_path_normalization_is_ignored_by_into_router() {
    let router = NestFactory::create::<AppModule>()
        .use_path_normalization(PathNormalization::TrimTrailingSlash)
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    // `use_path_normalization` is applied in `listen*` methods where the app is wrapped as a Service.
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn trailing_slash_without_path_normalization_is_not_found() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn global_prefix_wraps_controller_routes() {
    let router = NestFactory::create::<AppModule>()
        .set_global_prefix("platform")
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/platform/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn global_prefix_normalizes_slashes() {
    let router = NestFactory::create::<AppModule>()
        .set_global_prefix("/platform/")
        .into_router();

    let ok = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/platform/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(ok.status(), StatusCode::OK);

    let not_found = router
        .oneshot(
            Request::builder()
                .uri("//platform//v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn uri_versioning_wraps_existing_routes() {
    let router = NestFactory::create::<AppModule>()
        .enable_uri_versioning("edge")
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/edge/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn uri_versioning_normalizes_slashes() {
    let router = NestFactory::create::<AppModule>()
        .enable_uri_versioning("/edge/")
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/edge/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn cors_permissive_allows_any_origin() {
    let router = NestFactory::create::<AppModule>()
        .enable_cors(CorsOptions::permissive())
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static("*"))
    );
}

#[tokio::test]
async fn cors_allowlist_allows_configured_origin() {
    let router = NestFactory::create::<AppModule>()
        .enable_cors(
            CorsOptions::builder()
                .allow_origins(["https://app.example.com"])
                .allow_methods(["GET", "POST"])
                .allow_headers(["content-type", "authorization"])
                .allow_credentials(true)
                .max_age_secs(3600)
                .build(),
        )
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("OPTIONS")
                .header(header::ORIGIN, "https://app.example.com")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static("https://app.example.com"))
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
        Some(&HeaderValue::from_static("true"))
    );
}

#[tokio::test]
async fn security_headers_default_are_applied() {
    let router = NestFactory::create::<AppModule>()
        .use_security_headers(SecurityHeaders::default())
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(
        response.headers().get("x-content-type-options"),
        Some(&HeaderValue::from_static("nosniff"))
    );
    assert_eq!(
        response.headers().get("x-frame-options"),
        Some(&HeaderValue::from_static("DENY"))
    );
    assert_eq!(
        response.headers().get("referrer-policy"),
        Some(&HeaderValue::from_static("strict-origin-when-cross-origin"))
    );
}

#[tokio::test]
async fn security_headers_custom_values_are_applied() {
    let router = NestFactory::create::<AppModule>()
        .use_security_headers(
            SecurityHeaders::default()
                .content_security_policy("default-src 'self'")
                .hsts("max-age=31536000; includeSubDomains"),
        )
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(
        response.headers().get("content-security-policy"),
        Some(&HeaderValue::from_static("default-src 'self'"))
    );
    assert_eq!(
        response.headers().get("strict-transport-security"),
        Some(&HeaderValue::from_static(
            "max-age=31536000; includeSubDomains"
        ))
    );
}

#[tokio::test]
async fn rate_limit_returns_too_many_requests() {
    let router = NestFactory::create::<AppModule>()
        .use_rate_limit(
            RateLimitOptions::builder()
                .max_requests(1)
                .window_secs(60)
                .build(),
        )
        .into_router();

    let first = router
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
    assert_eq!(first.status(), StatusCode::OK);

    let second = router
        .oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn body_limit_rejects_large_payload() {
    let router = NestFactory::create::<AppModule>()
        .use_body_limit(4)
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/echo")
                .method("POST")
                .body(Body::from("123456789"))
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn request_timeout_returns_gateway_timeout() {
    let router = NestFactory::create::<AppModule>()
        .use_request_timeout(std::time::Duration::from_millis(30))
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/slow")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
}

#[tokio::test]
async fn concurrency_limit_without_load_shed_queues_second_request() {
    let router = NestFactory::create::<AppModule>()
        .use_concurrency_limit(1)
        .into_router();

    let svc1 = router.clone();
    let req1 = tokio::spawn(async move {
        svc1.oneshot(
            Request::builder()
                .uri("/v1/api/slow")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request")
    });

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let svc2 = router.clone();
    let req2 = tokio::spawn(async move {
        svc2.oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request")
    });

    let r1 = req1.await.expect("task join");
    let r2 = req2.await.expect("task join");
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::OK);
}

#[tokio::test]
async fn load_shed_with_concurrency_limit_rejects_when_overloaded() {
    let router = NestFactory::create::<AppModule>()
        .use_concurrency_limit(1)
        .use_load_shed()
        .into_router();

    let svc1 = router.clone();
    let req1 = tokio::spawn(async move {
        svc1.oneshot(
            Request::builder()
                .uri("/v1/api/slow")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request")
    });

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let svc2 = router.clone();
    let req2 = tokio::spawn(async move {
        svc2.oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request")
    });

    let _ = req1.await.expect("task join");
    let r2 = req2.await.expect("task join");
    assert_eq!(r2.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn production_errors_sanitize_5xx_message() {
    let router = NestFactory::create::<AppModule>()
        .enable_production_errors()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/internal-error")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body();
    let bytes = to_bytes(body, 64 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["message"], "An unexpected error occurred");
    let msg = v["message"].as_str().unwrap();
    assert!(!msg.contains("secret"));
}

#[test]
#[serial(env)]
fn runtime_is_production_reads_nestrs_env_first() {
    let _a = EnvGuard::set("NESTRS_ENV", "production");
    let _b = EnvGuard::set("APP_ENV", "development");
    assert!(runtime_is_production());
}

#[test]
#[serial(env)]
fn runtime_is_production_false_when_unset() {
    let _clear = ClearProductionEnvGuard::new();
    assert!(!runtime_is_production());
}

#[tokio::test]
#[serial(env)]
async fn production_errors_from_env_sanitizes_when_nestrs_env_production() {
    let _g = EnvGuard::set("NESTRS_ENV", "production");
    let router = NestFactory::create::<AppModule>()
        .enable_production_errors_from_env()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/internal-error")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body();
    let bytes = to_bytes(body, 64 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["message"], "An unexpected error occurred");
}

#[tokio::test]
async fn use_request_id_sets_response_header() {
    const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
    let router = NestFactory::create::<AppModule>()
        .use_request_id()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let id = response
        .headers()
        .get(X_REQUEST_ID)
        .expect("x-request-id header");
    let s = id.to_str().expect("header utf-8");
    assert!(!s.is_empty());
}

#[tokio::test]
async fn additional_http_exceptions_emit_expected_status_and_error_label() {
    let cases = [
        (
            PaymentRequiredException::new("pay"),
            StatusCode::PAYMENT_REQUIRED,
            "Payment Required",
        ),
        (
            MethodNotAllowedException::new("nope"),
            StatusCode::METHOD_NOT_ALLOWED,
            "Method Not Allowed",
        ),
        (
            NotAcceptableException::new("nope"),
            StatusCode::NOT_ACCEPTABLE,
            "Not Acceptable",
        ),
        (
            RequestTimeoutException::new("slow"),
            StatusCode::REQUEST_TIMEOUT,
            "Request Timeout",
        ),
        (GoneException::new("gone"), StatusCode::GONE, "Gone"),
        (
            PayloadTooLargeException::new("big"),
            StatusCode::PAYLOAD_TOO_LARGE,
            "Payload Too Large",
        ),
        (
            UnsupportedMediaTypeException::new("bad-content-type"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Unsupported Media Type",
        ),
        (
            NotImplementedException::new("todo"),
            StatusCode::NOT_IMPLEMENTED,
            "Not Implemented",
        ),
    ];

    for (ex, status, label) in cases {
        let res = ex.into_response();
        assert_eq!(res.status(), status);
        let bytes = to_bytes(res.into_body(), 16 * 1024)
            .await
            .expect("read body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["statusCode"], status.as_u16());
        assert_eq!(v["error"], label);
    }
}

#[tokio::test]
async fn use_request_context_extractor_sees_method_path_and_query() {
    let router = NestFactory::create::<AppModule>()
        .use_request_context()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/ctx?x=1")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024)
        .await
        .expect("read body");
    let s = String::from_utf8_lossy(&body);
    assert_eq!(s, "GET|/v1/api/ctx?x=1|");
}

#[tokio::test]
async fn use_request_context_with_request_id_fills_request_id_field() {
    const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
    let router = NestFactory::create::<AppModule>()
        .use_request_context()
        .use_request_id()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/ctx")
                .method("GET")
                .header(X_REQUEST_ID, "incoming-rid")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024)
        .await
        .expect("read body");
    let s = String::from_utf8_lossy(&body);
    assert_eq!(s, "GET|/v1/api/ctx|incoming-rid");
}

#[tokio::test]
async fn use_request_decompression_decodes_gzip_request_body() {
    let router = NestFactory::create::<AppModule>()
        .use_request_decompression()
        .into_router();

    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(b"hello-from-gzip").unwrap();
    let gz = enc.finish().unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/api/echo")
                .header(header::CONTENT_ENCODING, "gzip")
                .body(Body::from(gz))
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 32 * 1024)
        .await
        .expect("read body");
    assert_eq!(body.as_ref(), b"hello-from-gzip");
}

#[tokio::test]
async fn use_request_decompression_rejects_unsupported_content_encoding() {
    let router = NestFactory::create::<AppModule>()
        .use_request_decompression()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/api/echo")
                .header(header::CONTENT_ENCODING, "br")
                .body(Body::from("not-brotli"))
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn use_compression_sets_content_encoding_gzip_when_accepted() {
    let router = NestFactory::create::<AppModule>()
        .use_compression()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/compressible")
                .method("GET")
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let enc = response
        .headers()
        .get(header::CONTENT_ENCODING)
        .expect("gzip content-encoding when client accepts gzip");
    assert_eq!(enc.as_bytes(), b"gzip");
}

#[tokio::test]
async fn use_request_id_preserves_incoming_header() {
    const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
    let router = NestFactory::create::<AppModule>()
        .use_request_id()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .header(X_REQUEST_ID, "client-correlation-abc")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(X_REQUEST_ID).unwrap().as_bytes(),
        b"client-correlation-abc"
    );
}

#[tokio::test]
async fn enable_default_fallback_returns_nest_json_404() {
    let router = NestFactory::create::<AppModule>()
        .enable_default_fallback()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/no-such-route-nestrs")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body();
    let bytes = to_bytes(body, 16 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["statusCode"], 404);
    assert_eq!(v["error"], "Not Found");
    let msg = v["message"].as_str().expect("message string");
    assert!(msg.contains("GET"), "message={msg}");
    assert!(msg.contains("/no-such-route-nestrs"), "message={msg}");
}

#[tokio::test]
async fn health_check_returns_ok_json() {
    let router = NestFactory::create::<AppModule>()
        .enable_health_check("/health")
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/health")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body();
    let bytes = to_bytes(body, 16 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["status"], "ok");
}

#[tokio::test]
async fn health_check_is_not_under_global_prefix_or_uri_version() {
    let router = NestFactory::create::<AppModule>()
        .set_global_prefix("platform")
        .enable_uri_versioning("edge")
        .enable_health_check("/health")
        .into_router();

    let health = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(health.status(), StatusCode::OK);

    let app_ok = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/platform/edge/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(app_ok.status(), StatusCode::OK);

    let wrong = router
        .oneshot(
            Request::builder()
                .uri("/platform/edge/health")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(wrong.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn health_check_custom_path() {
    let router = NestFactory::create::<AppModule>()
        .enable_health_check("/livez")
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/livez")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial(metrics)]
async fn metrics_scrape_is_root_and_prometheus_text() {
    let router = NestFactory::create::<AppModule>()
        .set_global_prefix("api")
        .enable_metrics("/metrics")
        .into_router();

    let _ = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    let response = router
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let ctype = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ctype.starts_with("text/plain"),
        "unexpected content-type: {ctype}"
    );
    let body = response.into_body();
    let text = String::from_utf8(
        to_bytes(body, 512 * 1024)
            .await
            .expect("read body")
            .to_vec(),
    )
    .expect("utf8");
    assert!(text.contains("http_requests_total"), "{text}");
    assert!(text.contains("http_request_duration_seconds"), "{text}");
    assert!(text.contains("http_requests_in_flight"), "{text}");
}

#[tokio::test]
#[serial(metrics)]
async fn metrics_scrape_not_under_global_prefix() {
    let router = NestFactory::create::<AppModule>()
        .set_global_prefix("api")
        .enable_metrics("/metrics")
        .into_router();

    let nested = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/metrics")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(nested.status(), StatusCode::NOT_FOUND);

    let root = router
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(root.status(), StatusCode::OK);
}

#[tokio::test]
#[serial(metrics)]
async fn request_tracing_composes_with_request_id_and_metrics() {
    let router = NestFactory::create::<AppModule>()
        .use_request_id()
        .enable_metrics("/metrics")
        .use_request_tracing(RequestTracingOptions::builder().skip_paths(["/metrics"]))
        .into_router();

    let api = router
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
    assert_eq!(api.status(), StatusCode::OK);
    assert!(api.headers().get("x-request-id").is_some());

    let metrics = router
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(metrics.status(), StatusCode::OK);
}

#[tokio::test]
async fn readiness_empty_indicators_returns_ok() {
    let router = NestFactory::create::<AppModule>()
        .enable_readiness_check("/ready", Vec::<std::sync::Arc<dyn HealthIndicator>>::new())
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/ready")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body();
    let bytes = to_bytes(body, 32 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["status"], "ok");
}

#[tokio::test]
async fn readiness_all_up_returns_200() {
    let router = NestFactory::create::<AppModule>()
        .enable_readiness_check(
            "/ready",
            [std::sync::Arc::new(ReadinessAlwaysUp) as std::sync::Arc<dyn HealthIndicator>],
        )
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/ready")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body();
    let bytes = to_bytes(body, 32 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["status"], "ok");
    assert_eq!(v["info"]["always_up"]["status"], "up");
}

#[tokio::test]
async fn readiness_one_down_returns_503() {
    let router = NestFactory::create::<AppModule>()
        .enable_readiness_check(
            "/ready",
            [
                std::sync::Arc::new(ReadinessAlwaysUp) as std::sync::Arc<dyn HealthIndicator>,
                std::sync::Arc::new(ReadinessAlwaysDown) as std::sync::Arc<dyn HealthIndicator>,
            ],
        )
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/ready")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body();
    let bytes = to_bytes(body, 32 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["status"], "error");
    assert_eq!(v["error"]["always_down"]["status"], "down");
}

#[tokio::test]
async fn readiness_is_not_under_global_prefix() {
    let router = NestFactory::create::<AppModule>()
        .set_global_prefix("platform")
        .enable_readiness_check(
            "/ready",
            [std::sync::Arc::new(ReadinessAlwaysUp) as std::sync::Arc<dyn HealthIndicator>],
        )
        .into_router();

    let ready = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ready")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(ready.status(), StatusCode::OK);

    let nested = router
        .oneshot(
            Request::builder()
                .uri("/platform/ready")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");
    assert_eq!(nested.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_guards_allow_when_can_activate_ok() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/guarded-ok")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body();
    let bytes = to_bytes(body, 1024).await.expect("read body");
    assert_eq!(std::str::from_utf8(&bytes).expect("utf8"), "guarded-ok");
}

#[tokio::test]
async fn route_guards_return_403_when_can_activate_denies() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/guarded-deny")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = response.into_body();
    let bytes = to_bytes(body, 4096).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["statusCode"], 403);
    assert_eq!(v["message"], "denied by test guard");
}

#[tokio::test]
async fn use_global_layer_applies_outermost_tower_layer() {
    const HDR: &str = "x-nestrs-global-test";

    let router = NestFactory::create::<AppModule>()
        .use_global_layer(|r| {
            r.layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static(HDR),
                HeaderValue::from_static("from-global-layer"),
            ))
        })
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(HDR),
        Some(&HeaderValue::from_static("from-global-layer"))
    );
}

#[derive(Clone, Default)]
struct RewriteInternalErrorFilter;

#[async_trait]
impl ExceptionFilter for RewriteInternalErrorFilter {
    async fn catch(&self, mut ex: HttpException) -> axum::response::Response {
        if ex.status == StatusCode::INTERNAL_SERVER_ERROR {
            ex.message = "filtered-body".to_string();
        }
        ex.into_response()
    }
}

#[tokio::test]
async fn global_exception_filter_runs_before_outer_middleware() {
    let router = NestFactory::create::<AppModule>()
        .use_global_exception_filter(RewriteInternalErrorFilter)
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/internal-error")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body();
    let bytes = to_bytes(body, 64 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["message"], "filtered-body");
    assert_ne!(v["message"], "secret-internal-detail");
}

#[tokio::test]
async fn development_errors_keep_5xx_message() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/api/internal-error")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body();
    let bytes = to_bytes(body, 64 * 1024).await.expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert!(v["message"]
        .as_str()
        .unwrap()
        .contains("secret-internal-detail"));
}
