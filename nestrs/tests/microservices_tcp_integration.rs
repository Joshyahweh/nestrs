#![cfg(feature = "microservices")]

use nestrs::prelude::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static EVENT_HITS: AtomicUsize = AtomicUsize::new(0);

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

#[module(
    controllers = [HttpController],
    providers = [AppState, UserHandler, GuardedHandler],
    microservices = [UserHandler, GuardedHandler]
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
