//! TCP microservice transport hardening — bounded frames, idle-connection
//! drop, client RPC timeouts, and pipelining preservation.
//!
//! Audit finding: "TCP unbounded frame OOM / no timeouts". A peer that
//! streams bytes without ever sending a newline used to grow server (or
//! client) memory without bound; idle connections and unanswered RPCs hung
//! forever. These tests pin the hardened behavior.

#![cfg(feature = "microservices")]

use nestrs::microservices::{
    TcpMicroserviceOptions, TcpMicroserviceServer, TcpTransport, TcpTransportOptions, Transport,
    TransportError, MAX_FRAME_BYTES,
};
use std::net::{Ipv4Addr, SocketAddr};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

/// Reserve an ephemeral port, then release it for the server to bind.
async fn pick_free_addr() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind ephemeral");
    listener.local_addr().expect("local addr")
}

async fn wait_tcp(addr: SocketAddr) {
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("tcp server never came up on {addr}");
}

/// Spawn a bare TCP microservice (empty handler list is enough for the
/// wire-level tests here) and return its address plus a shutdown trigger.
async fn spawn_server() -> (SocketAddr, oneshot::Sender<()>) {
    let addr = pick_free_addr().await;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let bind_addr = addr;
    tokio::spawn(async move {
        let server = TcpMicroserviceServer::new(TcpMicroserviceOptions::new(bind_addr), vec![]);
        server
            .listen_with_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("tcp microservice");
    });
    wait_tcp(addr).await;
    (addr, shutdown_tx)
}

#[tokio::test]
async fn oversized_frame_is_rejected_and_connection_dropped() {
    let (addr, shutdown) = spawn_server().await;

    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    // Exactly one byte past the cap, no newline: the server accumulates
    // every byte, trips the cap, replies with a generic error frame, and
    // closes. Memory can no longer grow with the frame.
    let payload = vec![b'a'; MAX_FRAME_BYTES + 1];
    stream
        .write_all(&payload)
        .await
        .expect("write oversized frame");

    let mut reader = BufReader::new(stream);
    let mut rejection = String::new();
    let n = reader
        .read_line(&mut rejection)
        .await
        .expect("read rejection");
    assert!(n > 0, "server must reply with an error frame");
    assert!(
        rejection.contains("frame rejected"),
        "unexpected rejection: {rejection}"
    );

    // The connection is dropped after the rejection, not left open.
    let mut trailing = String::new();
    let n = reader.read_line(&mut trailing).await.expect("read EOF");
    assert_eq!(n, 0, "connection must be closed after a rejected frame");

    let _ = shutdown.send(());
}

#[tokio::test]
async fn pipelined_frames_are_not_swallowed() {
    let (addr, shutdown) = spawn_server().await;

    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    // Two complete frames in a single write: the capped reader must leave
    // the second frame buffered for the next loop iteration, not discard
    // it as leftover bytes.
    let frames = concat!(
        r#"{"id":"1","kind":"send","pattern":"p","payload":{}}"#,
        "\n",
        r#"{"id":"2","kind":"send","pattern":"p","payload":{}}"#,
        "\n",
    );
    stream
        .write_all(frames.as_bytes())
        .await
        .expect("write frames");

    let mut reader = BufReader::new(stream);
    for expected in ["1", "2"] {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .expect("read response line");
        assert!(n > 0, "expected a response for frame {expected}");
        let v: serde_json::Value = serde_json::from_str(&line).expect("valid JSON response");
        assert_eq!(v["id"], expected, "responses must be in order: {line}");
        // No handlers registered: both frames still get a structured error,
        // proving the second frame was parsed and dispatched.
        assert_eq!(v["ok"], false, "line: {line}");
        assert_eq!(
            v["error"]["message"], "no microservice handler for pattern `p`",
            "line: {line}"
        );
    }

    let _ = shutdown.send(());
}

#[tokio::test(start_paused = true)]
async fn idle_connection_is_dropped_after_timeout() {
    let (addr, shutdown) = spawn_server().await;

    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    // Send nothing. The server's idle timeout must drop the connection
    // rather than holding a task per silent peer. Under the paused clock the
    // runtime auto-advances to the timeout the moment every task is blocked.
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = reader.read_line(&mut line).await.expect("read");
    assert_eq!(n, 0, "idle connection must be dropped (EOF)");

    let _ = shutdown.send(());
}

#[tokio::test(start_paused = true)]
async fn send_times_out_when_server_never_responds() {
    // A hostile or broken server: accepts, consumes the request, and then
    // stays silent forever. The client's whole-RPC budget must fail the call
    // instead of hanging.
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind silent server");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let (mut conn, _) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 4096];
        let _ = conn.read(&mut buf).await;
        std::future::pending::<()>().await;
    });

    let transport = TcpTransport::new(TcpTransportOptions::new(addr));
    let err: TransportError = transport
        .send_json("some.pattern", serde_json::json!({ "x": 1 }))
        .await
        .expect_err("a silent server must time the RPC out");
    assert!(
        err.message.contains("timed out"),
        "expected a timeout error, got: {}",
        err.message
    );
}
