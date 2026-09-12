#![cfg(feature = "ws")]

use nestrs::prelude::*;
use nestrs::ws::{
    RegistryAwareWsGateway, WsCanActivate, WsClient, WsEvent, WsGateway, WsGuardError, WsHandshake,
    WsIncomingInterceptor, WsPipeError, WsPipeTransform, WS_ERROR_EVENT,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;

static INTERCEPT_VIA: std::sync::Mutex<&'static str> = std::sync::Mutex::new("none");

// Marker provider: only resolvable from a registry that registered it.
#[derive(Default)]
#[injectable]
struct GuardTicket;

// Default-constructed => denies. `resolve(registry)` pulls a provider and
// admits — so a pong frame proves the registry-aware dispatch ran
// `resolve` with the app registry rather than `Default::default()`.
#[derive(Default)]
struct RegistryBoundGuard {
    resolved: bool,
}

#[nestrs::async_trait]
impl WsCanActivate for RegistryBoundGuard {
    fn resolve(registry: &nestrs::core::ProviderRegistry) -> Self {
        let _ticket: Arc<GuardTicket> = registry.get();
        Self { resolved: true }
    }

    async fn can_activate_ws(
        &self,
        _handshake: &WsHandshake,
        _event: &str,
        _payload: &Value,
    ) -> Result<(), WsGuardError> {
        if self.resolved {
            Ok(())
        } else {
            Err(WsGuardError::unauthorized("guard was not DI-resolved"))
        }
    }
}

// Stamps which construction path ran into the payload it transforms.
struct RegistryBoundPipe {
    via: &'static str,
}

impl Default for RegistryBoundPipe {
    fn default() -> Self {
        Self { via: "default" }
    }
}

#[nestrs::async_trait]
impl WsPipeTransform for RegistryBoundPipe {
    fn resolve(_registry: &nestrs::core::ProviderRegistry) -> Self {
        Self { via: "registry" }
    }

    async fn transform(&self, _event: &str, mut payload: Value) -> Result<Value, WsPipeError> {
        payload["pipe_via"] = Value::from(self.via);
        Ok(payload)
    }
}

// Records which construction path ran into a static.
struct RegistryBoundInterceptor {
    via: &'static str,
}

impl Default for RegistryBoundInterceptor {
    fn default() -> Self {
        Self { via: "default" }
    }
}

#[nestrs::async_trait]
impl WsIncomingInterceptor for RegistryBoundInterceptor {
    fn resolve(_registry: &nestrs::core::ProviderRegistry) -> Self {
        Self { via: "registry" }
    }

    async fn before_handle(&self, _handshake: &WsHandshake, _event: &str, _payload: &Value) {
        *INTERCEPT_VIA.lock().unwrap() = self.via;
    }
}

#[derive(Default)]
#[injectable]
struct DiGateway;

#[ws_routes]
impl DiGateway {
    #[subscribe_message("ping")]
    #[use_ws_interceptors(RegistryBoundInterceptor)]
    #[use_ws_guards(RegistryBoundGuard)]
    #[use_ws_pipes(RegistryBoundPipe)]
    async fn ping(&self, client: nestrs::ws::WsClient, payload: Value) {
        let _ = client.emit("pong", payload);
    }
}

fn mock_client() -> (
    WsClient,
    mpsc::UnboundedReceiver<axum::extract::ws::Message>,
) {
    let (tx, rx) = mpsc::unbounded_channel::<axum::extract::ws::Message>();
    let client = WsClient::from_tx_for_test(tx, WsHandshake::default());
    (client, rx)
}

async fn recv_event(
    rx: &mut mpsc::UnboundedReceiver<axum::extract::ws::Message>,
) -> WsEvent<Value> {
    let msg = rx.recv().await.expect("recv");
    let axum::extract::ws::Message::Text(text) = msg else {
        panic!("expected text frame");
    };
    serde_json::from_str(&text).expect("frame json")
}

#[tokio::test]
async fn registry_and_plain_ws_dispatch_split_construction_paths() {
    *INTERCEPT_VIA.lock().unwrap() = "none";

    // 1. Registry-aware dispatch: resolve guard/pipe/interceptor through a
    //    registry that actually holds the marker provider.
    let mut registry = nestrs::core::ProviderRegistry::new();
    registry.register::<GuardTicket>();
    registry.register::<DiGateway>();
    let gateway = registry.get::<DiGateway>();

    let (client, mut rx) = mock_client();
    gateway
        .on_message_with_registry(client, "ping", json!({ "hello": "world" }), &registry)
        .await;

    // The guard only admits when DI-resolved, so the pong frame proves
    // `resolve` ran with the app registry.
    let frame = recv_event(&mut rx).await;
    assert_eq!(frame.event, "pong");
    assert_eq!(
        frame.data.get("hello").and_then(|v| v.as_str()),
        Some("world")
    );
    assert_eq!(
        frame.data.get("pipe_via").and_then(|v| v.as_str()),
        Some("registry")
    );
    assert_eq!(*INTERCEPT_VIA.lock().unwrap(), "registry");

    // 2. Plain dispatch (a hand-mounted `Arc<T>` with no registry) keeps
    //    `Default` construction: the guard denies, the interceptor still
    //    observes via its Default instance.
    let (client, mut rx) = mock_client();
    gateway.on_message(client, "ping", json!({})).await;

    let frame = recv_event(&mut rx).await;
    assert_eq!(frame.event, WS_ERROR_EVENT);
    assert_eq!(
        frame.data.get("message").and_then(|v| v.as_str()),
        Some("guard was not DI-resolved")
    );
    assert_eq!(*INTERCEPT_VIA.lock().unwrap(), "default");
}
