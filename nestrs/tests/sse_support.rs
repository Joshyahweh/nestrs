use axum::body::to_bytes;
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller]
struct SseController;

#[routes(state = AppState)]
impl SseController {
    #[get("/events")]
    #[sse]
    async fn events() -> impl IntoResponse {
        let stream = futures_util::stream::iter([Ok::<_, std::convert::Infallible>(
            nestrs::sse::Event::default().data("heartbeat"),
        )]);
        nestrs::sse::Sse::new(stream)
    }
}

#[module(controllers = [SseController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn sse_route_sets_event_stream_content_type() {
    let app = TestingModule::builder::<AppModule>().compile().await;
    let client = app.http_client();
    let res = client.get("/events").send().await;

    assert_eq!(res.status(), 200);
    let ct = res
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("text/event-stream"),
        "unexpected content-type: {ct}"
    );

    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("heartbeat"), "unexpected body: {body}");
}
