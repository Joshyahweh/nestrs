#![cfg(feature = "microservices")]

use nestrs::prelude::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static EVENT_HITS: AtomicUsize = AtomicUsize::new(0);
static WILDCARD_EVENT_HITS: AtomicUsize = AtomicUsize::new(0);
static INTERCEPT_VIA: std::sync::Mutex<&'static str> = std::sync::Mutex::new("none");

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/api", version = "v1")]
struct HttpController;

#[routes(state = AppState)]
impl HttpController {
    #[get("/")]
    async fn root() -> &'static str {
        "ok"
    }
}

#[dto]
struct GetUserReq {
    #[validate(range(min = 0))]
    id: i64,
}

#[dto]
struct UserRes {
    #[IsString]
    name: String,
}

#[dto]
struct UserCreatedEvent {
    id: i64,
}

#[derive(Default)]
struct RejectFortyTwoGuard;

#[nestrs::async_trait]
impl nestrs::microservices::MicroCanActivate for RejectFortyTwoGuard {
    async fn can_activate_micro(
        &self,
        _pattern: &str,
        payload: &serde_json::Value,
    ) -> Result<(), nestrs::microservices::TransportError> {
        if payload.get("id").and_then(|v| v.as_i64()) == Some(42) {
            return Err(nestrs::microservices::TransportError::new(
                "blocked-by-guard",
            ));
        }
        Ok(())
    }
}

// —— DI-backed cross-cutting concerns ——

// Marker provider: only resolvable from a registry that registered it.
#[derive(Default)]
#[injectable]
struct GuardTicket;

// Default-constructed => denies. `resolve(registry)` pulls a provider and
// admits — so a successful round-trip proves the registry-aware dispatch
// ran `resolve` with the app registry rather than `Default::default()`.
#[derive(Default)]
struct RegistryBoundGuard {
    resolved: bool,
}

#[nestrs::async_trait]
impl nestrs::microservices::MicroCanActivate for RegistryBoundGuard {
    fn resolve(registry: &nestrs::core::ProviderRegistry) -> Self {
        let _ticket: std::sync::Arc<GuardTicket> = registry.get();
        Self { resolved: true }
    }

    async fn can_activate_micro(
        &self,
        _pattern: &str,
        _payload: &serde_json::Value,
    ) -> Result<(), nestrs::microservices::TransportError> {
        if self.resolved {
            Ok(())
        } else {
            Err(nestrs::microservices::TransportError::new(
                "guard was not DI-resolved",
            ))
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
impl nestrs::microservices::MicroPipeTransform for RegistryBoundPipe {
    fn resolve(_registry: &nestrs::core::ProviderRegistry) -> Self {
        Self { via: "registry" }
    }

    async fn transform_micro(
        &self,
        _pattern: &str,
        mut payload: serde_json::Value,
    ) -> Result<serde_json::Value, nestrs::microservices::TransportError> {
        payload["pipe_via"] = serde_json::Value::from(self.via);
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
impl nestrs::microservices::MicroIncomingInterceptor for RegistryBoundInterceptor {
    fn resolve(_registry: &nestrs::core::ProviderRegistry) -> Self {
        Self { via: "registry" }
    }

    async fn before_handle_micro(&self, _pattern: &str, _payload: &serde_json::Value) {
        *INTERCEPT_VIA.lock().unwrap() = self.via;
    }
}

#[derive(Default)]
#[injectable]
struct DiHandler;

#[micro_routes]
impl DiHandler {
    #[message_pattern("di.echo")]
    #[use_micro_interceptors(RegistryBoundInterceptor)]
    #[use_micro_guards(RegistryBoundGuard)]
    #[use_micro_pipes(RegistryBoundPipe)]
    async fn echo(
        &self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, HttpException> {
        Ok(payload)
    }
}

#[derive(Default)]
#[injectable]
struct UserHandler;

#[micro_routes]
impl UserHandler {
    #[message_pattern("user.get")]
    async fn get_user(&self, req: GetUserReq) -> Result<UserRes, HttpException> {
        if req.id == 0 {
            return Err(BadRequestException::new("id must be non-zero"));
        }
        Ok(UserRes {
            name: format!("user-{}", req.id),
        })
    }

    #[event_pattern("user.created")]
    async fn on_user_created(&self, _evt: UserCreatedEvent) {
        EVENT_HITS.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Default)]
#[injectable]
struct GuardedHandler;

#[micro_routes]
impl GuardedHandler {
    #[message_pattern("guard.probe")]
    #[use_micro_guards(RejectFortyTwoGuard)]
    async fn probe(&self, req: GetUserReq) -> UserRes {
        UserRes {
            name: format!("p-{}", req.id),
        }
    }
}

// Wildcard handler patterns. `wc.*` is declared BEFORE the literal `wc.exact`
// on purpose: literal arms are emitted ahead of wildcard arms regardless of
// declaration order, so `wc.exact` must hit the literal handler below.
#[derive(Default)]
#[injectable]
struct WildcardHandler;

#[micro_routes]
impl WildcardHandler {
    #[message_pattern("wc.*")]
    async fn star(&self, _payload: serde_json::Value) -> Result<UserRes, HttpException> {
        Ok(UserRes {
            name: "star".to_string(),
        })
    }

    #[message_pattern("wc.exact")]
    async fn exact(&self, _payload: serde_json::Value) -> Result<UserRes, HttpException> {
        Ok(UserRes {
            name: "exact".to_string(),
        })
    }

    #[message_pattern("deep.one.>")]
    async fn deep(&self, _payload: serde_json::Value) -> Result<UserRes, HttpException> {
        Ok(UserRes {
            name: "deep".to_string(),
        })
    }

    #[event_pattern("wc.events.>")]
    async fn on_any_wc_event(&self, _payload: serde_json::Value) {
        WILDCARD_EVENT_HITS.fetch_add(1, Ordering::Relaxed);
    }
}

#[module(
    controllers = [HttpController],
    providers = [AppState, UserHandler, GuardedHandler, WildcardHandler, GuardTicket, DiHandler],
    microservices = [UserHandler, GuardedHandler, WildcardHandler, DiHandler]
)]
struct AppModule;

async fn pick_free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind ephemeral");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    port
}

async fn wait_tcp(addr: SocketAddr) {
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("server did not start: {addr}");
}

#[tokio::test]
async fn tcp_microservice_send_round_trips_and_http_exception_serializes_details() {
    EVENT_HITS.store(0, Ordering::Relaxed);

    let ms_port = pick_free_port().await;
    let http_port = pick_free_port().await;
    let ms_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), ms_port);

    let app = NestFactory::create_microservice::<AppModule>(
        nestrs::microservices::TcpMicroserviceOptions::new(ms_addr),
    )
    .also_listen_http(http_port);

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        app.listen_with_shutdown(async move {
            let _ = rx.await;
        })
        .await;
    });

    // Wait for both listeners.
    wait_tcp(ms_addr).await;
    wait_tcp(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), http_port)).await;

    // Microservice send ok.
    let transport = nestrs::microservices::TcpTransport::new(
        nestrs::microservices::TcpTransportOptions::new(ms_addr),
    );
    let proxy = nestrs::microservices::ClientProxy::new(std::sync::Arc::new(transport));
    let res: UserRes = proxy
        .send("user.get", &GetUserReq { id: 7 })
        .await
        .expect("send ok");
    assert_eq!(res.name, "user-7");

    // #[use_micro_guards] on #[message_pattern]
    let res: UserRes = proxy
        .send("guard.probe", &GetUserReq { id: 2 })
        .await
        .expect("guard.probe ok");
    assert_eq!(res.name, "p-2");
    let err = proxy
        .send::<GetUserReq, UserRes>("guard.probe", &GetUserReq { id: 42 })
        .await
        .expect_err("guard should block");
    assert_eq!(err.message, "blocked-by-guard");

    // Microservice send error: HttpException ⇒ TransportError details.
    let err = proxy
        .send::<GetUserReq, UserRes>("user.get", &GetUserReq { id: 0 })
        .await
        .expect_err("send should fail");
    let details = err.details.expect("details");
    assert_eq!(details["type"], "HttpException");
    assert_eq!(details["statusCode"], 400);

    // Microservice emit increments counter.
    proxy
        .emit("user.created", &UserCreatedEvent { id: 1 })
        .await
        .expect("emit ok");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(EVENT_HITS.load(Ordering::Relaxed) >= 1);

    // HTTP server responds.
    let mut stream = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, http_port))
        .await
        .expect("connect http");
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream
        .write_all(b"GET /v1/api HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("write");
    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await.expect("read");
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(
        head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200"),
        "unexpected response head: {head}"
    );

    let _ = tx.send(());
    let _ = join.await;
}

#[tokio::test]
async fn tcp_microservice_wildcard_patterns_match_with_literal_priority() {
    WILDCARD_EVENT_HITS.store(0, Ordering::Relaxed);

    let ms_port = pick_free_port().await;
    let ms_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), ms_port);

    let app = NestFactory::create_microservice::<AppModule>(
        nestrs::microservices::TcpMicroserviceOptions::new(ms_addr),
    );

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        app.listen_with_shutdown(async move {
            let _ = rx.await;
        })
        .await;
    });
    wait_tcp(ms_addr).await;

    let transport = nestrs::microservices::TcpTransport::new(
        nestrs::microservices::TcpTransportOptions::new(ms_addr),
    );
    let proxy = nestrs::microservices::ClientProxy::new(std::sync::Arc::new(transport));

    // `*` matches exactly one token: any single-token tail under `wc.` hits.
    let res: UserRes = proxy
        .send::<serde_json::Value, UserRes>("wc.anything", &serde_json::json!({}))
        .await
        .expect("wc.* should match wc.anything");
    assert_eq!(res.name, "star");

    // The literal `wc.exact` arm wins over the `wc.*` wildcard even though
    // the wildcard was declared first.
    let res: UserRes = proxy
        .send::<serde_json::Value, UserRes>("wc.exact", &serde_json::json!({}))
        .await
        .expect("wc.exact should match");
    assert_eq!(res.name, "exact");

    // `>` matches one or more trailing tokens...
    let res: UserRes = proxy
        .send::<serde_json::Value, UserRes>("deep.one.tail.tokens", &serde_json::json!({}))
        .await
        .expect("deep.one.> should match deep.one.tail.tokens");
    assert_eq!(res.name, "deep");

    // ...but not zero trailing tokens.
    let err = proxy
        .send::<serde_json::Value, UserRes>("deep.one", &serde_json::json!({}))
        .await
        .expect_err("`>` requires at least one trailing token");
    assert_eq!(err.message, "no microservice handler for pattern `deep.one`");

    // Too many tokens for a `*` pattern.
    let err = proxy
        .send::<serde_json::Value, UserRes>("wc.a.b", &serde_json::json!({}))
        .await
        .expect_err("wc.* must not match two-token tails");
    assert_eq!(err.message, "no microservice handler for pattern `wc.a.b`");

    // Event patterns support wildcards too.
    proxy
        .emit("wc.events.user.created", &UserCreatedEvent { id: 5 })
        .await
        .expect("emit ok");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(WILDCARD_EVENT_HITS.load(Ordering::Relaxed) >= 1);

    let _ = tx.send(());
    let _ = join.await;
}

#[tokio::test]
async fn tcp_microservice_cross_cutting_resolves_via_registry() {
    *INTERCEPT_VIA.lock().unwrap() = "none";

    let ms_port = pick_free_port().await;
    let ms_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), ms_port);

    let app = NestFactory::create_microservice::<AppModule>(
        nestrs::microservices::TcpMicroserviceOptions::new(ms_addr),
    );

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        app.listen_with_shutdown(async move {
            let _ = rx.await;
        })
        .await;
    });
    wait_tcp(ms_addr).await;

    let transport = nestrs::microservices::TcpTransport::new(
        nestrs::microservices::TcpTransportOptions::new(ms_addr),
    );
    let proxy = nestrs::microservices::ClientProxy::new(std::sync::Arc::new(transport));

    // The guard only admits when DI-resolved (Default denies), so a
    // successful round-trip proves the registry-aware dispatch resolved it
    // through the app's registry.
    let res: serde_json::Value = proxy
        .send("di.echo", &serde_json::json!({ "hello": "world" }))
        .await
        .expect("guard must have been resolved from the registry");

    // The pipe stamps its construction path into the payload.
    assert_eq!(
        res.get("pipe_via").and_then(|v| v.as_str()),
        Some("registry")
    );
    assert_eq!(res.get("hello").and_then(|v| v.as_str()), Some("world"));
    // The interceptor observed the message via its resolved instance.
    assert_eq!(*INTERCEPT_VIA.lock().unwrap(), "registry");

    let _ = tx.send(());
    let _ = join.await;
}
