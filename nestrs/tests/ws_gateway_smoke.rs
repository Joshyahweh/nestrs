#![cfg(feature = "ws")]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct AppState;

#[ws_gateway(path = "/ws")]
#[derive(Default)]
#[injectable]
struct TestGateway;

#[ws_routes]
impl TestGateway {
    #[subscribe_message("ping")]
    async fn ping(&self, client: nestrs::ws::WsClient, payload: serde_json::Value) {
        let _ = client.emit("pong", payload);
    }
}

#[module(controllers = [TestGateway], providers = [AppState, TestGateway])]
struct AppModule;

#[tokio::test]
async fn ws_gateway_route_is_registered() {
    let router = NestFactory::create::<AppModule>().into_router();

    // No websocket upgrade headers ⇒ should NOT be 404.
    let res = router
        .oneshot(
            Request::builder()
                .uri("/ws")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    assert_ne!(res.status(), StatusCode::NOT_FOUND);
}
