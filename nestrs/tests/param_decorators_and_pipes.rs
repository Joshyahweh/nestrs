use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use nestrs::prelude::*;
use std::net::SocketAddr;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[dto]
struct SignupDto {
    #[IsEmail]
    email: String,
}

#[dto]
struct SearchQuery {
    #[IsString]
    #[MinLength(3)]
    term: String,
}

#[dto]
struct ItemParams {
    #[validate(range(min = 1))]
    id: i64,
}

#[controller(prefix = "/t", version = "v1")]
struct TestController;

#[routes(state = AppState)]
impl TestController {
    #[post("/signup")]
    #[use_pipes(ValidationPipe)]
    async fn signup(#[param::body] dto: SignupDto) -> &'static str {
        let _ = dto;
        "ok"
    }

    #[get("/search")]
    #[use_pipes(ValidationPipe)]
    async fn search(#[param::query] q: SearchQuery) -> String {
        q.term
    }

    #[get("/items/:id")]
    #[use_pipes(ValidationPipe)]
    async fn item(#[param::param] p: ItemParams) -> String {
        p.id.to_string()
    }

    #[get("/ip")]
    async fn ip(#[param::ip] ip: std::net::IpAddr) -> String {
        ip.to_string()
    }
}

#[module(controllers = [TestController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn param_body_with_validation_pipe_returns_422_on_invalid() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/t/signup")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"email":"not-an-email"}"#))
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["statusCode"], 422);
    assert_eq!(v["message"], "Validation failed");
    assert!(v["errors"].is_array());
}

#[tokio::test]
async fn param_query_with_validation_pipe_returns_422_on_invalid() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/t/search?term=ab")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn param_path_with_validation_pipe_returns_422_on_invalid() {
    let router = NestFactory::create::<AppModule>().into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/t/items/0")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn param_ip_reads_connect_info_when_present() {
    use nestrs::axum::extract::connect_info::MockConnectInfo;

    let router = NestFactory::create::<AppModule>()
        .into_router()
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 4321))));

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/t/ip")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read");
    assert_eq!(String::from_utf8_lossy(&bytes), "127.0.0.1");
}
