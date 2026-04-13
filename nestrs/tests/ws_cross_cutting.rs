#![cfg(feature = "ws")]

use axum::http::HeaderValue;
use nestrs::async_trait;
use nestrs::ws::{
    WsCanActivate, WsGuardError, WsHandshake, WsIncomingInterceptor, WsPipeError, WsPipeTransform,
};
use serde_json::json;

#[derive(Default)]
struct TokenHeaderGuard;

#[async_trait]
impl WsCanActivate for TokenHeaderGuard {
    async fn can_activate_ws(
        &self,
        handshake: &WsHandshake,
        _event: &str,
        _payload: &serde_json::Value,
    ) -> Result<(), WsGuardError> {
        let ok = handshake
            .headers()
            .get("x-ws-token")
            .and_then(|v| v.to_str().ok())
            == Some("secret");
        if ok {
            Ok(())
        } else {
            Err(WsGuardError::forbidden("missing token"))
        }
    }
}

#[tokio::test]
async fn ws_guard_reads_upgrade_headers() {
    let mut map = axum::http::HeaderMap::new();
    map.insert("x-ws-token", HeaderValue::from_static("secret"));
    let hs = WsHandshake::new(map);
    assert!(TokenHeaderGuard::default()
        .can_activate_ws(&hs, "ping", &json!({}))
        .await
        .is_ok());
}

#[derive(Default)]
struct NoopLog;

#[async_trait]
impl WsIncomingInterceptor for NoopLog {
    async fn before_handle(
        &self,
        _handshake: &WsHandshake,
        _event: &str,
        _payload: &serde_json::Value,
    ) {
    }
}

#[derive(Default)]
struct PassthroughPipe;

#[async_trait]
impl WsPipeTransform for PassthroughPipe {
    async fn transform(
        &self,
        _event: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, WsPipeError> {
        Ok(payload)
    }
}

// —— Macro expansion smoke (guards / pipes / interceptors on `subscribe_message`) ——

use nestrs::prelude::*;

#[ws_gateway(path = "/ws-guarded")]
#[derive(Default)]
#[injectable]
struct GuardedWsGateway;

#[ws_routes]
impl GuardedWsGateway {
    #[subscribe_message("hi")]
    #[use_ws_interceptors(NoopLog)]
    #[use_ws_guards(TokenHeaderGuard)]
    #[use_ws_pipes(PassthroughPipe)]
    async fn hi(&self, _client: nestrs::ws::WsClient) {}
}

#[module(controllers = [GuardedWsGateway], providers = [GuardedWsGateway])]
struct GuardedWsApp;

#[test]
fn ws_routes_with_guards_pipes_interceptors_compose() {
    let _router = NestFactory::create::<GuardedWsApp>().into_router();
}
