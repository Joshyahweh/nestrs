use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[derive(Default)]
struct HeaderInterceptor;

#[async_trait]
impl Interceptor for HeaderInterceptor {
    async fn intercept(
        &self,
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        let mut res = next.run(req).await;
        res.headers_mut().insert(
            "x-nestrs-interceptor",
            axum::http::HeaderValue::from_static("ok"),
        );
        res
    }
}

#[derive(Default)]
struct RewriteNotFoundMessage;

#[async_trait]
impl ExceptionFilter for RewriteNotFoundMessage {
    async fn catch(&self, ex: HttpException) -> axum::response::Response {
        let mut rewritten = ex;
        rewritten.message = "rewritten".to_string();
        rewritten.into_response()
    }
}

#[controller(prefix = "/x", version = "v1")]
struct DemoController;

#[routes(state = AppState)]
impl DemoController {
    #[get("/ok")]
    #[use_interceptors(HeaderInterceptor)]
    async fn ok() -> &'static str {
        "ok"
    }

    #[get("/err")]
    #[use_filters(RewriteNotFoundMessage)]
    async fn err() -> Result<&'static str, HttpException> {
        Err(NotFoundException::new("nope"))
    }
}

#[module(controllers = [DemoController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn interceptors_and_filters_apply_per_route() {
    let router = NestFactory::create::<AppModule>().into_router();

    let ok = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/x/ok")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    assert_eq!(ok.status(), StatusCode::OK);
    assert_eq!(
        ok.headers().get("x-nestrs-interceptor"),
        Some(&axum::http::HeaderValue::from_static("ok"))
    );

    let err = router
        .oneshot(
            Request::builder()
                .uri("/v1/x/err")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    assert_eq!(err.status(), StatusCode::NOT_FOUND);
    let bytes = to_bytes(err.into_body(), 4096).await.expect("body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["message"], "rewritten");
}

