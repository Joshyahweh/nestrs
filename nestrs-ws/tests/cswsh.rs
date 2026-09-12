//! Cross-Site WebSocket Hijacking (CSWSH) defence tests.
//!
//! Verifies that [`ws_route_with_security`] and
//! [`ws_route_with_guards_and_security`] reject upgrades whose `Origin`
//! header is not on the configured allowlist. Real loopback: each test
//! spawns an axum server and connects with tokio-tungstenite, asserting
//! on the actual HTTP response the upgrade returns.

use futures_util::{SinkExt, StreamExt};
use nestrs_ws::{
    ws_route_with_guards_and_security, ws_route_with_security, CloseCode, WsCanActivate, WsClient,
    WsGateway, WsGuardError, WsHandshake, WsSecurityConfig, WS_ERROR_EVENT,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::{Request, StatusCode},
    Error as TError, Message as TMessage,
};

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Default)]
struct EchoGateway;

#[async_trait::async_trait]
impl WsGateway for EchoGateway {
    async fn on_message(&self, client: WsClient, event: &str, payload: Value) {
        let _ = client.emit(event, payload);
    }
}

async fn send_event(stream: &mut WsStream, event: &str, data: Value) {
    let frame = serde_json::to_string(&json!({ "event": event, "data": data })).unwrap();
    stream.send(TMessage::Text(frame)).await.expect("send");
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
            TMessage::Close(frame) => return (frame.map(|f| u16::from(f.code)), errors),
            _ => {}
        }
    }
    (None, errors)
}

fn allowed_cfg() -> WsSecurityConfig {
    WsSecurityConfig::allow_origins(["https://allowed.example.com"])
}

/// Build a client request with the supplied Origin (or no Origin at all).
fn request_with_origin(addr: std::net::SocketAddr, origin: Option<&str>) -> Request<()> {
    let mut req =
        IntoClientRequest::into_client_request(format!("ws://{addr}/ws")).expect("static URI");
    if let Some(o) = origin {
        req.headers_mut()
            .insert("Origin", o.parse().expect("Origin header value"));
    }
    req
}

// ---------------------------------------------------------------------------
// Allow-off (legacy) behaviour
// ---------------------------------------------------------------------------

#[tokio::test]
async fn allow_off_accepts_every_origin() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ws",
        ws_route_with_security(Arc::new(EchoGateway), WsSecurityConfig::allow_off()),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let request = request_with_origin(addr, Some("https://anywhere.example.com"));
    let (mut stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("allow_off accepts any origin");
    send_event(&mut stream, "ping", json!("hi")).await;
    // Echo arrives, then we initiate the close from the client side.
    while let Some(Ok(msg)) = stream.next().await {
        if let TMessage::Text(text) = msg {
            let v: Value = serde_json::from_str(&text).expect("json");
            if v["event"] == "ping" {
                stream.send(TMessage::Close(None)).await.expect("close");
                let _ = read_until_close(&mut stream).await;
                return;
            }
        }
    }
    panic!("echo never arrived");
}

// ---------------------------------------------------------------------------
// CSWSH-safe entry point with explicit allowlist
// ---------------------------------------------------------------------------

#[tokio::test]
async fn allowed_origin_connects_and_messages_flow() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ws",
        ws_route_with_security(Arc::new(EchoGateway), allowed_cfg()),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let request = request_with_origin(addr, Some("https://allowed.example.com"));
    let (mut stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("allowlisted origin must connect");
    send_event(&mut stream, "ping", json!("hello")).await;
    // Echo arrives — the connection is live past the security check.
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

#[tokio::test]
async fn disallowed_origin_gets_403() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ws",
        ws_route_with_security(Arc::new(EchoGateway), allowed_cfg()),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let request = request_with_origin(addr, Some("https://evil.example.com"));
    let err = tokio_tungstenite::connect_async(request)
        .await
        .expect_err("disallowed origin must be rejected");
    match err {
        TError::Http(resp) => assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "disallowed origin ⇒ HTTP 403"
        ),
        other => panic!("expected HTTP error, got {other:?}"),
    }
}

#[tokio::test]
async fn null_origin_is_always_rejected_when_allowlist_present() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ws",
        ws_route_with_security(Arc::new(EchoGateway), allowed_cfg()),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let request = request_with_origin(addr, Some("null"));
    let err = tokio_tungstenite::connect_async(request)
        .await
        .expect_err("Origin: null must be rejected");
    match err {
        TError::Http(resp) => assert_eq!(resp.status(), StatusCode::FORBIDDEN),
        other => panic!("expected HTTP error, got {other:?}"),
    }
}

#[tokio::test]
async fn no_origin_is_accepted_by_default() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ws",
        ws_route_with_security(Arc::new(EchoGateway), allowed_cfg()),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let request = request_with_origin(addr, None);
    let (mut stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("bare-origin upgrade accepted by default");
    // Drop immediately — we just need to confirm the upgrade succeeded.
    let _ = stream.send(TMessage::Close(None)).await;
}

#[tokio::test]
async fn require_origin_rejects_bare_origin_upgrades() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let cfg = allowed_cfg().require_origin(true);
    let app = axum::Router::new().route("/ws", ws_route_with_security(Arc::new(EchoGateway), cfg));
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let request = request_with_origin(addr, None);
    let err = tokio_tungstenite::connect_async(request)
        .await
        .expect_err("require_origin + no Origin ⇒ 403");
    match err {
        TError::Http(resp) => assert_eq!(resp.status(), StatusCode::FORBIDDEN),
        other => panic!("expected HTTP error, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Combined security + upgrade-time guard
// ---------------------------------------------------------------------------

struct DenyUpgradeGuard;

impl Default for DenyUpgradeGuard {
    fn default() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl WsCanActivate for DenyUpgradeGuard {
    async fn can_activate_ws(
        &self,
        _handshake: &WsHandshake,
        _event: &str,
        _payload: &Value,
    ) -> Result<(), WsGuardError> {
        Err(WsGuardError::unauthorized("denied by upgrade guard"))
    }
}

#[tokio::test]
async fn security_runs_before_guard_and_short_circuits() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ws",
        ws_route_with_guards_and_security(
            Arc::new(EchoGateway),
            vec![Arc::new(DenyUpgradeGuard)],
            allowed_cfg(),
        ),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    // Disallowed origin ⇒ 403 (security runs first).
    let bad = request_with_origin(addr, Some("https://evil.example.com"));
    let err = tokio_tungstenite::connect_async(bad)
        .await
        .expect_err("security check rejects before guard");
    match err {
        TError::Http(resp) => assert_eq!(resp.status(), StatusCode::FORBIDDEN),
        other => panic!("expected HTTP error, got {other:?}"),
    }

    // Allowed origin, but guard rejects ⇒ in-protocol 1008 close.
    let mut req =
        IntoClientRequest::into_client_request(format!("ws://{addr}/ws")).expect("static URI");
    req.headers_mut().insert(
        "Origin",
        "https://allowed.example.com".parse().expect("Origin"),
    );
    let (mut stream, _) = tokio_tungstenite::connect_async(req)
        .await
        .expect("allowed origin connects");
    let (code, _errors) = read_until_close(&mut stream).await;
    assert_eq!(code, Some(1008), "guard rejection ⇒ 1008 PolicyViolation");
}

// ---------------------------------------------------------------------------
// CloseCode sanity for the new entry points
// ---------------------------------------------------------------------------

#[test]
fn policy_violation_close_code_is_rfc_compliant() {
    assert_eq!(CloseCode::PolicyViolation.as_u16(), 1008);
    assert_eq!(CloseCode::from_u16(1008), Some(CloseCode::PolicyViolation));
}
