//! WebSocket gateway primitives for `nestrs` (NestJS `WebSocketGateway` analogue).
//!
//! ## Wire format
//!
//! Default frames are JSON: `{ "event": "name", "data": <json> }`.
//!
//! ## Adapters (Nest vs Axum)
//!
//! This crate uses **Axum’s `WebSocketUpgrade`** (browser `WebSocket` / RFC 6455). Nest’s **Socket.IO**
//! transport is a different protocol; for Socket.IO on Axum see the **[`socketioxide`](https://crates.io/crates/socketioxide)**
//! ecosystem, or keep event-shaped JSON and document your client contract.
//!
//! The `nestrs-macros` crate can generate dispatch via `#[ws_routes]` + `#[subscribe_message("...")]`.
//! Optional cross-cutting attributes: **`#[use_ws_interceptors(...)]`**, **`#[use_ws_guards(...)]`**,
//! **`#[use_ws_pipes(...)]`** (see crate docs on `nestrs`).
//!
//! ## Errors vs HTTP “exception filters”
//!
//! WebSocket messages are **not** Axum HTTP responses. [`NestApplication::use_global_exception_filter`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_global_exception_filter)
//! and [`ExceptionFilter`](https://docs.rs/nestrs/latest/nestrs/trait.ExceptionFilter.html) **do not** run on
//! JSON frames. Instead, failures are sent to the client on the event name [`WS_ERROR_EVENT`]
//! (`"error"`) with JSON bodies:
//!
//! - **Guards** ([`WsGuardError`]): `statusCode`, `message`, `error` (see [`WsGuardError::to_json`]).
//! - **Pipes** ([`WsPipeError`]): same top-level keys as guards for pipe failures (`statusCode` 400).
//! - **Unknown event** (generated `#[ws_routes]` default arm): `event`, `message` (`"unknown event"`).
//! - **Invalid typed payload** (deserialize into handler DTO): `event`, `message`, `details` (string).
//! - **Wire parse errors** in [`serve_socket`] (malformed `{event,data}`): `message` only,
//!   **followed by a Close frame with code 1003** (`CloseCode::UnsupportedData`) — a client
//!   that cannot speak the agreed wire format is closed instead of being left connected.
//!
//! Treat these as the WebSocket analogue of Nest’s gateway exception filters: **centralize** by
//! wrapping [`WsGateway::on_message`] or using shared guard/pipe types; there is no separate
//! `WsExceptionFilter` trait in-core today.
//!
//! ## Origin allowlist (CSWSH)
//!
//! [`ws_route`] and [`ws_route_with_guards`] do **not** validate the `Origin`
//! header on the HTTP upgrade — they accept browsers from any origin. This is
//! the textbook **Cross-Site WebSocket Hijacking (CSWSH)** footgun: an
//! attacker page can open a WebSocket to your gateway and call protected
//! handlers as the victim. New code should mount via [`ws_route_with_security`]
//! or [`ws_route_with_guards_and_security`] with an explicit [`WsSecurityConfig`]
//! that names every allowed origin. The legacy entry points are retained for
//! callers behind a trusted reverse proxy that already validates Origin.

/// Event name used for server→client error frames (guards, pipes, unknown event, bad payloads).
pub const WS_ERROR_EVENT: &str = "error";

pub mod adapters;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use futures_util::{FutureExt, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsEvent<T> {
    pub event: String,
    pub data: T,
}

#[derive(Debug)]
pub enum WsSendError {
    Serialize(String),
    Closed,
}

/// RFC 6455 §7.4.1 close codes used by the runtime (plus the 4000–4999
/// application range). WebSocket layers in NestJS land close on
/// `close(code, reason)`; here a [`CloseCode`] + reason becomes an
/// axum [`Message::Close`] frame via [`WsClient::close`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseCode {
    /// 1000 — normal, intentional closure.
    Normal,
    /// 1001 — endpoint going away (server shutdown, redeploy).
    GoingAway,
    /// 1003 — peer sent data the endpoint cannot accept (bad wire format).
    UnsupportedData,
    /// 1008 — policy violation (auth failure, rejected message).
    PolicyViolation,
    /// 1011 — endpoint hit an unexpected condition (handler panic, 5xx guard).
    InternalError,
    /// 1012 — service restarting.
    ServiceRestart,
    /// 1013 — temporary condition (overload); retry later.
    TryAgainLater,
    /// 4000–4999 — private / application-defined use.
    Application(u16),
}

impl CloseCode {
    pub fn as_u16(self) -> u16 {
        match self {
            Self::Normal => 1000,
            Self::GoingAway => 1001,
            Self::UnsupportedData => 1003,
            Self::PolicyViolation => 1008,
            Self::InternalError => 1011,
            Self::ServiceRestart => 1012,
            Self::TryAgainLater => 1013,
            Self::Application(code) => code,
        }
    }

    /// Inverse of [`CloseCode::as_u16`]. Only the known codes and the
    /// 4000–4999 range round-trip; anything else (including reserved
    /// 1004/1005/1006/1015 and the 2000–2999 gap) is `None`.
    pub fn from_u16(code: u16) -> Option<Self> {
        match code {
            1000 => Some(Self::Normal),
            1001 => Some(Self::GoingAway),
            1003 => Some(Self::UnsupportedData),
            1008 => Some(Self::PolicyViolation),
            1011 => Some(Self::InternalError),
            1012 => Some(Self::ServiceRestart),
            1013 => Some(Self::TryAgainLater),
            4000..=4999 => Some(Self::Application(code)),
            _ => None,
        }
    }

    /// Build the axum Close frame for this code.
    pub(crate) fn close_frame(self, reason: &str) -> Message {
        Message::Close(Some(axum::extract::ws::CloseFrame {
            code: self.as_u16(),
            reason: reason.to_owned().into(),
        }))
    }
}

/// Map a guard rejection to its close code: server-side failures (5xx) are
/// internal errors (1011); everything else — 401/403 and other 4xx policy
/// rejections — is a policy violation (1008).
fn close_code_for_guard(err: &WsGuardError) -> CloseCode {
    if err.status_code >= 500 {
        CloseCode::InternalError
    } else {
        CloseCode::PolicyViolation
    }
}

/// Headers (and related data) from the HTTP upgrade request, available on each [`WsClient`].
#[derive(Clone, Debug)]
pub struct WsHandshake {
    headers: Arc<HeaderMap>,
}

impl Default for WsHandshake {
    fn default() -> Self {
        Self {
            headers: Arc::new(HeaderMap::new()),
        }
    }
}

impl WsHandshake {
    pub fn new(headers: HeaderMap) -> Self {
        Self {
            headers: Arc::new(headers),
        }
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

/// Failure from a WebSocket [`WsCanActivate`] guard (emitted as an `error` event by generated code).
#[derive(Debug, Clone)]
pub struct WsGuardError {
    pub status_code: u16,
    pub message: String,
    pub error: &'static str,
}

impl WsGuardError {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status_code: 401,
            message: message.into(),
            error: "Unauthorized",
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status_code: 403,
            message: message.into(),
            error: "Forbidden",
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status_code: 400,
            message: message.into(),
            error: "Bad Request",
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "statusCode": self.status_code,
            "message": self.message,
            "error": self.error,
        })
    }
}

/// Authorization / policy hook before a message handler runs (Nest `WsGuard` analogue).
#[async_trait::async_trait]
pub trait WsCanActivate: Default + Send + Sync + 'static {
    async fn can_activate_ws(
        &self,
        handshake: &WsHandshake,
        event: &str,
        payload: &serde_json::Value,
    ) -> Result<(), WsGuardError>;
}

/// Object-safe, per-message guard for the **runtime** chain
/// ([`WsGateway::message_guards`]). [`WsCanActivate`] requires `Default`,
/// which makes it non-object-safe; runtime guards are supplied as
/// `Arc<dyn WsMessageGuard>` and run inside [`serve_socket`] on every
/// inbound message — opt-in defense in depth on top of the
/// `#[use_ws_guards(...)]` checks compiled into the macro dispatch.
///
/// Any [`WsCanActivate`] type automatically implements this trait.
#[async_trait::async_trait]
pub trait WsMessageGuard: Send + Sync + 'static {
    async fn can_activate_message(
        &self,
        handshake: &WsHandshake,
        event: &str,
        payload: &serde_json::Value,
    ) -> Result<(), WsGuardError>;
}

#[async_trait::async_trait]
impl<T: WsCanActivate> WsMessageGuard for T {
    async fn can_activate_message(
        &self,
        handshake: &WsHandshake,
        event: &str,
        payload: &serde_json::Value,
    ) -> Result<(), WsGuardError> {
        self.can_activate_ws(handshake, event, payload).await
    }
}

/// Ordered per-message guard chain run by [`serve_socket`] before each
/// message reaches the gateway. Empty by default; populate via
/// [`WsGateway::message_guards`]. The first rejection short-circuits.
#[derive(Clone, Default)]
pub struct WsGuardChain {
    guards: Vec<Arc<dyn WsMessageGuard>>,
}

impl WsGuardChain {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style push.
    pub fn with_guard(mut self, guard: Arc<dyn WsMessageGuard>) -> Self {
        self.guards.push(guard);
        self
    }

    pub fn push(&mut self, guard: Arc<dyn WsMessageGuard>) {
        self.guards.push(guard);
    }

    pub fn is_empty(&self) -> bool {
        self.guards.is_empty()
    }

    /// Run the chain in order against one inbound message.
    pub async fn run(
        &self,
        handshake: &WsHandshake,
        event: &str,
        payload: &serde_json::Value,
    ) -> Result<(), WsGuardError> {
        for guard in &self.guards {
            guard
                .can_activate_message(handshake, event, payload)
                .await?;
        }
        Ok(())
    }
}

/// Guard evaluated **once** on the HTTP upgrade request, before any
/// message flows ([`ws_route_with_guards`]). A rejection does not bounce
/// the HTTP request — the upgrade is accepted and the socket immediately
/// closed with **1008 Policy Violation**, so WS clients observe an
/// in-protocol rejection instead of an HTTP error they may not surface.
///
/// Any [`WsCanActivate`] type automatically implements this trait: it is
/// invoked with an empty event name and a `null` payload.
#[async_trait::async_trait]
pub trait WsUpgradeGuard: Send + Sync + 'static {
    async fn can_upgrade(&self, handshake: &WsHandshake) -> Result<(), WsGuardError>;
}

#[async_trait::async_trait]
impl<T: WsCanActivate> WsUpgradeGuard for T {
    async fn can_upgrade(&self, handshake: &WsHandshake) -> Result<(), WsGuardError> {
        self.can_activate_ws(handshake, "", &serde_json::Value::Null)
            .await
    }
}

/// Transform inbound JSON after guards (Nest `Pipe` analogue for payloads).
#[async_trait::async_trait]
pub trait WsPipeTransform: Default + Send + Sync + 'static {
    async fn transform(
        &self,
        event: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, WsPipeError>;
}

#[derive(Debug, Clone)]
pub struct WsPipeError {
    pub message: String,
}

impl WsPipeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "statusCode": 400,
            "message": self.message,
            "error": "Bad Request",
        })
    }
}

/// Observe inbound messages (logging / metrics); does not fail the pipeline.
#[async_trait::async_trait]
pub trait WsIncomingInterceptor: Default + Send + Sync + 'static {
    async fn before_handle(
        &self,
        handshake: &WsHandshake,
        event: &str,
        payload: &serde_json::Value,
    );
}

/// Handle for emitting messages back to a single connected client.
#[derive(Clone)]
pub struct WsClient {
    tx: mpsc::UnboundedSender<Message>,
    handshake: WsHandshake,
}

impl WsClient {
    /// Test-only constructor: lets a test build a `WsClient` from a raw
    /// `mpsc::UnboundedSender` so it can drive `emit_json` / `emit` and
    /// assert on the emitted frames. Not part of the public surface.
    #[doc(hidden)]
    pub fn from_tx_for_test(tx: mpsc::UnboundedSender<Message>, handshake: WsHandshake) -> Self {
        Self { tx, handshake }
    }

    pub fn handshake(&self) -> &WsHandshake {
        &self.handshake
    }

    pub fn emit<T: Serialize>(&self, event: &str, data: T) -> Result<(), WsSendError> {
        let payload =
            serde_json::to_value(data).map_err(|e| WsSendError::Serialize(e.to_string()))?;
        self.emit_json(event, payload)
    }

    pub fn emit_json(&self, event: &str, data: serde_json::Value) -> Result<(), WsSendError> {
        let frame = WsEvent {
            event: event.to_string(),
            data,
        };
        let text =
            serde_json::to_string(&frame).map_err(|e| WsSendError::Serialize(e.to_string()))?;
        self.tx
            .send(Message::Text(text))
            .map_err(|_| WsSendError::Closed)
    }

    /// Queue a Close frame with the given RFC 6455 code and reason. The
    /// frame is sent through the same channel as emitted events, so it
    /// lands after anything already queued. Gateway handlers call this to
    /// close the connection deliberately (shutdown → [`CloseCode::GoingAway`],
    /// rate limiting → [`CloseCode::TryAgainLater`], etc.); the runtime
    /// itself uses 1003/1008/1011 for decode, guard, and internal errors.
    pub fn close(&self, code: CloseCode, reason: &str) -> Result<(), WsSendError> {
        self.tx
            .send(code.close_frame(reason))
            .map_err(|_| WsSendError::Closed)
    }
}

#[async_trait::async_trait]
pub trait WsGateway: Send + Sync + 'static {
    async fn on_message(&self, client: WsClient, event: &str, payload: serde_json::Value);

    /// Opt-in per-message guard chain run by the shared runtime
    /// ([`serve_socket`]) before every `on_message`, on top of any
    /// `#[use_ws_guards(...)]` checks compiled into the dispatch.
    /// Default: empty chain. Return the guards as
    /// `Arc<dyn WsMessageGuard>`; rejections close the socket (guard
    /// status ≥ 500 → 1011, otherwise → 1008).
    fn message_guards(&self) -> WsGuardChain {
        WsGuardChain::new()
    }
}

/// Origin-policy for [`ws_route_with_security`] and
/// [`ws_route_with_guards_and_security`]. Defends against **Cross-Site
/// WebSocket Hijacking (CSWSH)**: a malicious page that opens a
/// WebSocket from the victim's browser to your gateway and issues calls
/// as the victim.
///
/// `allow_off()` (used by the legacy [`ws_route`] / [`ws_route_with_guards`]
/// entry points) accepts every Origin — match what browsers expect when
/// the gateway sits behind a trusted reverse proxy that already enforces
/// an allowlist. New browser-facing code should construct an explicit
/// allowlist via [`WsSecurityConfig::allow_origins`].
///
/// **Match semantics:** allowlist entries are compared against the
/// `Origin` header verbatim. A `null` Origin (sandboxed iframes,
/// `file://`, certain privacy contexts) is **always rejected** when any
/// allowlist entry is configured. When the upgrade carries no Origin
/// header (CLI tools, server-to-server clients), the request is
/// **accepted by default**; set [`WsSecurityConfig::require_origin`]
/// to reject bare-origin upgrades.
#[derive(Debug, Clone)]
pub struct WsSecurityConfig {
    allowed_origins: Vec<String>,
    require_origin: bool,
}

impl WsSecurityConfig {
    /// Disable Origin enforcement. Every browser/Origin is accepted.
    /// Use only when the gateway is fronted by a trusted reverse proxy
    /// that enforces its own allowlist.
    pub fn allow_off() -> Self {
        Self {
            allowed_origins: Vec::new(),
            require_origin: false,
        }
    }

    /// Build a config that accepts the listed `Origin` values verbatim.
    /// Pass full origins including scheme, e.g. `https://app.example.com`.
    /// A `null` Origin is always rejected.
    pub fn allow_origins<I, S>(origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed_origins: origins.into_iter().map(Into::into).collect(),
            require_origin: false,
        }
    }

    /// When `true`, upgrades with no `Origin` header are rejected
    /// (HTTP 403). Useful for browser-only gateways that should never
    /// accept raw server-to-server connections.
    pub fn require_origin(mut self, require: bool) -> Self {
        self.require_origin = require;
        self
    }

    /// Evaluate the policy against an upgrade request's `Origin` header.
    /// Returns `Ok(())` when the upgrade should proceed.
    fn check(&self, headers: &HeaderMap) -> Result<(), WsOriginError> {
        let origin = headers.get("origin").and_then(|v| v.to_str().ok());
        match (self.allowed_origins.as_slice(), origin) {
            // No allowlist configured — accept everything (proxy mode).
            ([], _) => Ok(()),
            // Allowlist configured, no Origin header — accept unless
            // the user explicitly required one.
            (_, None) if !self.require_origin => Ok(()),
            (_, None) => Err(WsOriginError::NoOrigin),
            // Allowlist configured, Origin is `null` — always reject.
            (_, Some("null")) => Err(WsOriginError::Denied("null".into())),
            // Allowlist configured, Origin must match verbatim.
            (_, Some(o)) if self.allowed_origins.iter().any(|a| a == o) => Ok(()),
            (_, Some(o)) => Err(WsOriginError::Denied(o.to_owned())),
        }
    }
}

/// Reason an upgrade was rejected by [`WsSecurityConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsOriginError {
    /// Upgrade carried no Origin header and the config required one.
    NoOrigin,
    /// Origin was present but not on the allowlist (the original
    /// value is preserved for logging).
    Denied(String),
}

impl WsOriginError {
    pub fn message(&self) -> String {
        match self {
            Self::NoOrigin => "Origin header required".into(),
            Self::Denied(o) => format!("Origin `{o}` not allowed"),
        }
    }
}

pub fn ws_route<G>(gateway: Arc<G>) -> axum::routing::MethodRouter
where
    G: WsGateway,
{
    ws_route_with_security(gateway, WsSecurityConfig::allow_off())
}

/// [`ws_route`] with an **Origin allowlist** that runs before the upgrade
/// is accepted. The CSWSH-safe replacement for [`ws_route`] — every
/// gateway exposed to browsers should mount via this entry point with a
/// non-empty [`WsSecurityConfig::allow_origins`] list.
pub fn ws_route_with_security<G>(
    gateway: Arc<G>,
    security: WsSecurityConfig,
) -> axum::routing::MethodRouter
where
    G: WsGateway,
{
    axum::routing::get(move |ws: WebSocketUpgrade, headers: HeaderMap| {
        let gw = gateway.clone();
        let security = security.clone();
        async move {
            if let Err(err) = security.check(&headers) {
                tracing::warn!(target: "nestrs::ws", "rejecting upgrade: {}", err.message());
                return axum::http::Response::builder()
                    .status(axum::http::StatusCode::FORBIDDEN)
                    .body(axum::body::Body::from(err.message()))
                    .expect("static response");
            }
            let handshake = WsHandshake::new(headers);
            ws.on_upgrade(move |socket| serve_socket(socket, gw, handshake))
        }
    })
}

/// [`ws_route`] with an **upgrade-time guard chain**: each guard runs
/// against the handshake before the socket is served; the first rejection
/// accepts the upgrade and immediately closes the socket with
/// [`CloseCode::PolicyViolation`] (1008) and the guard's message.
pub fn ws_route_with_guards<G>(
    gateway: Arc<G>,
    guards: Vec<Arc<dyn WsUpgradeGuard>>,
) -> axum::routing::MethodRouter
where
    G: WsGateway,
{
    ws_route_with_guards_and_security(gateway, guards, WsSecurityConfig::allow_off())
}

/// [`ws_route_with_guards`] with an **Origin allowlist**: the security
/// check runs first, then each upgrade guard runs against the handshake.
/// Security rejection is a hard 403 (no upgrade); guard rejection accepts
/// the upgrade and immediately closes the socket with
/// [`CloseCode::PolicyViolation`] so WS clients observe an in-protocol
/// rejection.
pub fn ws_route_with_guards_and_security<G>(
    gateway: Arc<G>,
    guards: Vec<Arc<dyn WsUpgradeGuard>>,
    security: WsSecurityConfig,
) -> axum::routing::MethodRouter
where
    G: WsGateway,
{
    axum::routing::get(move |ws: WebSocketUpgrade, headers: HeaderMap| {
        let gw = gateway.clone();
        let guards = guards.clone();
        let security = security.clone();
        async move {
            if let Err(err) = security.check(&headers) {
                tracing::warn!(target: "nestrs::ws", "rejecting upgrade: {}", err.message());
                return axum::http::Response::builder()
                    .status(axum::http::StatusCode::FORBIDDEN)
                    .body(axum::body::Body::from(err.message()))
                    .expect("static response");
            }
            let handshake = WsHandshake::new(headers);
            for guard in &guards {
                if let Err(err) = guard.can_upgrade(&handshake).await {
                    return ws.on_upgrade(move |socket| reject_upgrade(socket, err));
                }
            }
            ws.on_upgrade(move |socket| serve_socket(socket, gw, handshake))
        }
    })
}

/// Accept an upgrade, immediately Close with 1008 + the guard's message,
/// then (bounded) drain the peer's close echo before dropping.
async fn reject_upgrade(socket: WebSocket, err: WsGuardError) {
    let mut socket = socket;
    let close =
        CloseCode::PolicyViolation.close_frame(&format!("upgrade rejected: {}", err.message));
    if socket.send(close).await.is_ok() {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while let Some(Ok(msg)) = socket.next().await {
                if matches!(msg, Message::Close(_)) {
                    break;
                }
            }
        })
        .await;
    }
}

pub async fn serve_socket<G>(socket: WebSocket, gateway: Arc<G>, handshake: WsHandshake)
where
    G: WsGateway,
{
    // Install the W3C trace context from the handshake headers when present,
    // so gateway handlers (and anything running inside the connection task)
    // can read the caller's trace via `nestrs::core::current_trace_context()`.
    let headers = handshake.headers();
    let ctx = headers
        .get("traceparent")
        .and_then(|v| v.to_str().ok())
        .and_then(nestrs_core::parse_traceparent);
    if let Some(mut ctx) = ctx {
        ctx.tracestate = headers
            .get("tracestate")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        return nestrs_core::with_trace_context(
            ctx,
            serve_socket_inner(socket, gateway, handshake),
        )
        .await;
    }
    serve_socket_inner(socket, gateway, handshake).await;
}

async fn serve_socket_inner<G>(socket: WebSocket, gateway: Arc<G>, handshake: WsHandshake)
where
    G: WsGateway,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let client = WsClient {
        tx,
        handshake: handshake.clone(),
    };
    let chain = gateway.message_guards();

    let (mut ws_tx, mut ws_rx) = socket.split();

    let write_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    // A throwaway client sharing the outbound channel, for error/close
    // frames after the per-message client was consumed by `on_message`.
    fn error_client(client: &WsClient) -> WsClient {
        WsClient {
            tx: client.tx.clone(),
            handshake: client.handshake.clone(),
        }
    }

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                match serde_json::from_str::<WsEvent<serde_json::Value>>(&text) {
                    Ok(ev) => match chain.run(&client.handshake, &ev.event, &ev.data).await {
                        Err(err) => {
                            let c = error_client(&client);
                            let _ = c.emit(WS_ERROR_EVENT, err.to_json());
                            let code = close_code_for_guard(&err);
                            let _ = c.close(code, &err.message);
                            break;
                        }
                        Ok(()) => {
                            let c = error_client(&client);
                            let fut = gateway.on_message(c, ev.event.as_str(), ev.data);
                            if std::panic::AssertUnwindSafe(fut)
                                .catch_unwind()
                                .await
                                .is_err()
                            {
                                let c = error_client(&client);
                                let _ = c.emit(
                                    WS_ERROR_EVENT,
                                    serde_json::json!({ "message": "internal error" }),
                                );
                                let _ = c.close(CloseCode::InternalError, "internal error");
                                break;
                            }
                        }
                    },
                    Err(_) => {
                        let c = error_client(&client);
                        let _ = c.emit(
                            WS_ERROR_EVENT,
                            serde_json::json!({
                                "message": "invalid websocket payload (expected {event,data})"
                            }),
                        );
                        let _ = c.close(
                            CloseCode::UnsupportedData,
                            "invalid websocket payload (expected {event,data})",
                        );
                        break;
                    }
                }
            }
            Message::Binary(bin) => {
                if let Ok(text) = std::str::from_utf8(&bin) {
                    match serde_json::from_str::<WsEvent<serde_json::Value>>(text) {
                        Ok(ev) => match chain.run(&client.handshake, &ev.event, &ev.data).await {
                            Err(err) => {
                                let c = error_client(&client);
                                let _ = c.emit(WS_ERROR_EVENT, err.to_json());
                                let code = close_code_for_guard(&err);
                                let _ = c.close(code, &err.message);
                                break;
                            }
                            Ok(()) => {
                                let c = error_client(&client);
                                let fut = gateway.on_message(c, ev.event.as_str(), ev.data);
                                if std::panic::AssertUnwindSafe(fut)
                                    .catch_unwind()
                                    .await
                                    .is_err()
                                {
                                    let c = error_client(&client);
                                    let _ = c.emit(
                                        WS_ERROR_EVENT,
                                        serde_json::json!({ "message": "internal error" }),
                                    );
                                    let _ = c.close(CloseCode::InternalError, "internal error");
                                    break;
                                }
                            }
                        },
                        Err(_) => {
                            let c = error_client(&client);
                            let _ = c.emit(
                                WS_ERROR_EVENT,
                                serde_json::json!({
                                    "message": "invalid websocket payload (expected {event,data})"
                                }),
                            );
                            let _ = c.close(
                                CloseCode::UnsupportedData,
                                "invalid websocket payload (expected {event,data})",
                            );
                            break;
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    drop(client);
    let _ = write_task.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ws_client_emit_json_sends_event_frame() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let client = WsClient {
            tx,
            handshake: WsHandshake::default(),
        };

        client
            .emit_json("ping", serde_json::json!({ "ok": true }))
            .expect("send");

        let msg = rx.recv().await.expect("recv");
        let Message::Text(text) = msg else {
            panic!("expected text frame");
        };
        let ev: WsEvent<serde_json::Value> = serde_json::from_str(&text).expect("json");
        assert_eq!(ev.event, "ping");
        assert_eq!(ev.data["ok"], true);
    }
}
