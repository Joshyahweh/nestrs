use axum::body::{to_bytes, Body};
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller]
struct WebhookController;

#[routes(state = AppState)]
impl WebhookController {
    #[post("/webhook")]
    #[raw_body]
    async fn webhook(RawBody(bytes): RawBody) -> String {
        let text = std::str::from_utf8(bytes.as_ref()).unwrap_or("<non-utf8>");
        format!("{}:{text}", bytes.len())
    }
}

#[module(controllers = [WebhookController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn raw_body_reads_full_bytes() {
    let app = TestingModule::builder::<AppModule>().compile().await;
    let client = app.http_client();

    let res = client
        .post("/webhook")
        .body(Body::from("hello"))
        .send()
        .await;

    assert_eq!(res.status(), 200);
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(std::str::from_utf8(&bytes).unwrap(), "5:hello");
}
