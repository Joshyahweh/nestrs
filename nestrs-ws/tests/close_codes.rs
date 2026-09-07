//! Wave 3E.5 — RFC 6455 close codes + runtime per-message guard chain.
//!
//! Real loopback: each test spawns an axum server over [`ws_route`] /
//! [`ws_route_with_guards`] and connects with tokio-tungstenite, asserting
//! on the actual close frames the peer observes.

use futures_util::{SinkExt, StreamExt};
use nestrs_ws::{
    ws_route, ws_route_with_guards, CloseCode, WsCanActivate, WsClient, WsGateway, WsGuardChain,
    WsGuardError, WsHandshake, WsMessageGuard, WS_ERROR_EVENT,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message as TMessage;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn send_event(stream: &mut WsStream, event: &str, data: Value) {
    let frame = serde_json::to_string(&json!({ "event": event, "data": data })).expect("json");
    stream
        .send(TMessage::Text(frame))
        .await
        .expect("send frame");
}

/// Read frames until a Close arrives (or the stream ends); returns the
/// close code plus any error-frame JSON seen before it.
async fn read_until_close(stream: &mut WsStream) -> (Option<u16>, Vec<Value>) {
    let mut errors = Vec::new();
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            TMessage::Text(text) => {
                let v: Value = serde_json::from_str(&text).expect("frame json");
                if v.get("event").and_then(Value::as_str) == Some(WS_ERROR_EVENT) {
                    errors.push(v);
                }
            }
            TMessage::Close(frame) => {
                return (frame.map(|f| u16::from(f.code)), errors);
            }
            _ => {}
        }
    }
    (None, errors)
}

/// Echo gateway: bounces every event straight back at the caller.
#[derive(Default)]
struct EchoGateway;

#[async_trait::async_trait]
impl WsGateway for EchoGateway {
    async fn on_message(&self, client: WsClient, event: &str, payload: Value) {
        let _ = client.emit(event, payload);
    }
}

// ---------------------------------------------------------------------------
// CloseCode enum
// ---------------------------------------------------------------------------

#[test]
fn close_code_round_trips_rfc_6455_values() {
    for (code, expected) in [
        (CloseCode::Normal, 1000u16),
        (CloseCode::GoingAway, 1001),
        (CloseCode::UnsupportedData, 1003),
        (CloseCode::PolicyViolation, 1008),
        (CloseCode::InternalError, 1011),
        (CloseCode::ServiceRestart, 1012),
        (CloseCode::TryAgainLater, 1013),
    ] {
        assert_eq!(code.as_u16(), expected);
        assert_eq!(CloseCode::from_u16(expected), Some(code));
    }
    assert_eq!(CloseCode::Application(4321).as_u16(), 4321);
    assert_eq!(
        CloseCode::from_u16(4321),
        Some(CloseCode::Application(4321))
    );
    // Reserved / out-of-range codes do not round-trip.
    for raw in [1004, 1005, 1006, 1015, 2999, 5000] {
        assert_eq!(CloseCode::from_u16(raw), None, "{raw} must not parse");
    }
}

// ---------------------------------------------------------------------------
// Runtime close-code mapping
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gateway_initiated_close_emits_going_away() {
    struct ShutdownGateway;
    #[async_trait::async_trait]
    impl WsGateway for ShutdownGateway {
        async fn on_message(&self, client: WsClient, event: &str, _payload: Value) {
            if event == "shutdown" {
                let _ = client.close(CloseCode::GoingAway, "server shutting down");
            }
        }
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route("/ws", ws_route(Arc::new(ShutdownGateway)));
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let (mut stream, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    send_event(&mut stream, "shutdown", json!(null)).await;
    let (code, _) = read_until_close(&mut stream).await;
    assert_eq!(code, Some(1001), "forced shutdown ⇒ 1001 GoingAway");
}

#[tokio::test]
async fn decode_error_closes_with_1003_unsupported_data() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route("/ws", ws_route(Arc::new(EchoGateway)));
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let (mut stream, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    // A text frame that is not `{event, data}` JSON.
    stream
        .send(TMessage::Text("not json".into()))
        .await
        .expect("send");
    let (code, errors) = read_until_close(&mut stream).await;
    assert_eq!(
        code,
        Some(1003),
        "malformed wire frame ⇒ 1003 UnsupportedData"
    );
    assert_eq!(errors.len(), 1, "error frame precedes the close");
    assert!(errors[0]["data"]["message"]
        .as_str()
        .expect("message")
        .contains("invalid websocket payload"));
}

struct DenyGuard {
    status: u16,
}

#[async_trait::async_trait]
impl WsMessageGuard for DenyGuard {
    async fn can_activate_message(
        &self,
        _handshake: &WsHandshake,
        _event: &str,
        _payload: &Value,
    ) -> Result<(), WsGuardError> {
        Err(if self.status == 401 {
            WsGuardError::unauthorized("no token")
        } else {
            WsGuardError {
                status_code: self.status,
                message: "boom".into(),
                error: "Internal Server Error",
            }
        })
    }
}

static HANDLER_HITS: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
struct GuardedGateway;

#[async_trait::async_trait]
impl WsGateway for GuardedGateway {
    async fn on_message(&self, _client: WsClient, _event: &str, _payload: Value) {
        HANDLER_HITS.fetch_add(1, Ordering::SeqCst);
    }

    fn message_guards(&self) -> WsGuardChain {
        WsGuardChain::new().with_guard(Arc::new(DenyGuard { status: 401 }))
    }
}

#[tokio::test]
async fn per_message_guard_rejection_closes_1008_and_skips_handler() {
    HANDLER_HITS.store(0, Ordering::SeqCst);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route("/ws", ws_route(Arc::new(GuardedGateway)));
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let (mut stream, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    send_event(&mut stream, "hello", json!({})).await;
    let (code, errors) = read_until_close(&mut stream).await;
    assert_eq!(code, Some(1008), "auth failure ⇒ 1008 PolicyViolation");
    assert_eq!(errors[0]["data"]["statusCode"], 401);
    assert_eq!(HANDLER_HITS.load(Ordering::SeqCst), 0, "handler never ran");
}

#[tokio::test]
async fn server_side_guard_failure_closes_1011() {
    struct InternalGuard;
    #[async_trait::async_trait]
    impl WsMessageGuard for InternalGuard {
        async fn can_activate_message(
            &self,
            _handshake: &WsHandshake,
            _event: &str,
            _payload: &Value,
        ) -> Result<(), WsGuardError> {
            Err(WsGuardError {
                status_code: 500,
                message: "guard dependency down".into(),
                error: "Internal Server Error",
            })
        }
    }

    struct InternalGateway;
    #[async_trait::async_trait]
    impl WsGateway for InternalGateway {
        async fn on_message(&self, _client: WsClient, _event: &str, _payload: Value) {}
        fn message_guards(&self) -> WsGuardChain {
            WsGuardChain::new().with_guard(Arc::new(InternalGuard))
        }
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route("/ws", ws_route(Arc::new(InternalGateway)));
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let (mut stream, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    send_event(&mut stream, "hello", json!({})).await;
    let (code, _) = read_until_close(&mut stream).await;
    assert_eq!(code, Some(1011), "5xx guard ⇒ 1011 InternalError");
}

#[tokio::test]
async fn handler_panic_closes_1011() {
    struct PanicGateway;
    #[async_trait::async_trait]
    impl WsGateway for PanicGateway {
        async fn on_message(&self, _client: WsClient, event: &str, _payload: Value) {
            if event == "boom" {
                panic!("handler blew up");
            }
        }
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route("/ws", ws_route(Arc::new(PanicGateway)));
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let (mut stream, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    send_event(&mut stream, "boom", json!(null)).await;
    let (code, _) = read_until_close(&mut stream).await;
    assert_eq!(code, Some(1011), "handler panic ⇒ 1011 InternalError");
}

// ---------------------------------------------------------------------------
// Runtime guard-chain semantics
// ---------------------------------------------------------------------------

static FIRST_GUARD_HITS: AtomicUsize = AtomicUsize::new(0);

struct PassGuard;

#[async_trait::async_trait]
impl WsMessageGuard for PassGuard {
    async fn can_activate_message(
        &self,
        _handshake: &WsHandshake,
        _event: &str,
        _payload: &Value,
    ) -> Result<(), WsGuardError> {
        FIRST_GUARD_HITS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Default)]
struct ChainGateway;

#[async_trait::async_trait]
impl WsGateway for ChainGateway {
    async fn on_message(&self, _client: WsClient, _event: &str, _payload: Value) {
        HANDLER_HITS.fetch_add(1, Ordering::SeqCst);
    }

    // First guard passes (counted), second rejects — order + short-circuit.
    fn message_guards(&self) -> WsGuardChain {
        WsGuardChain::new()
            .with_guard(Arc::new(PassGuard))
            .with_guard(Arc::new(DenyGuard { status: 403 }))
    }
}

#[tokio::test]
async fn runtime_guard_chain_runs_in_order_and_short_circuits() {
    HANDLER_HITS.store(0, Ordering::SeqCst);
    FIRST_GUARD_HITS.store(0, Ordering::SeqCst);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route("/ws", ws_route(Arc::new(ChainGateway)));
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let (mut stream, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    send_event(&mut stream, "hello", json!({})).await;
    let (code, errors) = read_until_close(&mut stream).await;
    assert_eq!(code, Some(1008));
    assert_eq!(
        errors[0]["data"]["statusCode"], 403,
        "second guard's rejection surfaces"
    );
    assert_eq!(
        FIRST_GUARD_HITS.load(Ordering::SeqCst),
        1,
        "first guard ran in the runtime path"
    );
    assert_eq!(HANDLER_HITS.load(Ordering::SeqCst), 0);
}

// ---------------------------------------------------------------------------
// Upgrade-time guards
// ---------------------------------------------------------------------------

#[derive(Default)]
struct HeaderTokenGuard;

#[async_trait::async_trait]
impl WsCanActivate for HeaderTokenGuard {
    async fn can_activate_ws(
        &self,
        handshake: &WsHandshake,
        _event: &str,
        _payload: &Value,
    ) -> Result<(), WsGuardError> {
        // Blanket WsUpgradeGuard call: empty event + null payload.
        let ok = handshake
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            == Some("secret");
        if ok {
            Ok(())
        } else {
            Err(WsGuardError::unauthorized("missing token"))
        }
    }
}

#[tokio::test]
async fn upgrade_guard_rejects_with_1008_close() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ws",
        ws_route_with_guards(Arc::new(EchoGateway), vec![Arc::new(HeaderTokenGuard)]),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let (mut stream, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    let (code, _) = read_until_close(&mut stream).await;
    assert_eq!(
        code,
        Some(1008),
        "unauthorised upgrade ⇒ immediate 1008 close"
    );
}

#[tokio::test]
async fn upgrade_guard_allows_then_messages_flow() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ws",
        ws_route_with_guards(Arc::new(EchoGateway), vec![Arc::new(HeaderTokenGuard)]),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let mut request =
        tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(format!(
            "ws://{addr}/ws"
        ))
        .expect("request");
    request
        .headers_mut()
        .insert("authorization", "secret".parse().expect("header value"));
    let (mut stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("connect");
    send_event(&mut stream, "ping", json!("hello")).await;
    // Echo arrives — the connection is live past the guard.
    while let Some(Ok(msg)) = stream.next().await {
        if let TMessage::Text(text) = msg {
            let v: Value = serde_json::from_str(&text).expect("json");
            if v["event"] == "ping" {
                assert_eq!(v["data"], "hello");
                return;
            }
        }
    }
    panic!("echo never arrived");
}
