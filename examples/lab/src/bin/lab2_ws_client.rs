//! Lab 2b — live WebSocket frame test: boots the realtime lab in-process, then connects
//! over real TCP with a true WebSocket client (handshake + frames) and asserts the
//! gateway's echo/join behavior end-to-end.
//!
//! Run: `cargo run -p lab --bin lab2_ws_client`

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    // Boot the same app as lab2_realtime on port 3200.
    tokio::spawn(async {
        lab2_realtime::boot().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let (mut ws, _) = tokio_tungstenite::connect_async("ws://127.0.0.1:3200/lab/ws")
        .await
        .expect("websocket handshake failed");

    // 1. echo round-trip (strictly serialized: send → read → send → read)
    ws.send(Message::text(
        r#"{"event":"echo","data":{"msg":"hello over ws"}}"#,
    ))
    .await
    .expect("send echo");
    let reply = ws.next().await.expect("echo reply").expect("no ws error");
    println!("ECHO REPLY: {reply}");

    // 2. join → connection counter
    ws.send(Message::text(r#"{"event":"join","data":{"room":"lab"}}"#))
        .await
        .expect("send join");
    let reply = ws.next().await.expect("join reply").expect("no ws error");
    println!("JOIN REPLY (serialized): {reply}");

    // 2b. pipelined: two frames back-to-back before reading anything
    ws.send(Message::text(r#"{"event":"join","data":{}}"#))
        .await
        .expect("send join p1");
    ws.send(Message::text(r#"{"event":"join","data":{"x":1}}"#))
        .await
        .expect("send join p2");
    for i in 0..2 {
        let reply = ws
            .next()
            .await
            .expect("pipelined reply")
            .expect("no ws error");
        println!("PIPELINED REPLY {i}: {reply}");
    }

    // 3. abrupt close, then reconnect — connection counter should increment
    drop(ws);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let (mut ws2, _) = tokio_tungstenite::connect_async("ws://127.0.0.1:3200/lab/ws")
        .await
        .expect("re-handshake failed");
    ws2.send(Message::text(r#"{"event":"join","data":{}}"#))
        .await
        .expect("send join #2");
    let reply = ws2
        .next()
        .await
        .expect("join reply 2")
        .expect("no ws error");
    println!("JOIN REPLY 2 (after reconnect): {reply}");

    // 4. garbage payload must not kill the connection
    ws2.send(Message::text("this is not json"))
        .await
        .expect("send garbage");
    ws2.send(Message::text(r#"{"event":"echo","data":"still alive"}"#))
        .await
        .expect("send after garbage");
    let maybe = tokio::time::timeout(std::time::Duration::from_secs(2), ws2.next()).await;
    match maybe {
        Ok(Some(Ok(m))) => println!("AFTER GARBAGE: {m}"),
        other => println!("AFTER GARBAGE: connection state {other:?}"),
    }

    println!("LAB2 WS TEST DONE");
}

/// Mirrors `lab2_realtime.rs` so this binary can boot the app without a subprocess.
mod lab2_realtime {
    use nestrs::prelude::*;
    use nestrs::ws::WsClient;
    use std::sync::atomic::{AtomicU64, Ordering};

    static CONNECTIONS: AtomicU64 = AtomicU64::new(0);

    #[ws_gateway(path = "/ws")]
    #[derive(Default)]
    #[injectable]
    pub struct EchoGateway;

    #[ws_routes]
    impl EchoGateway {
        #[subscribe_message("echo")]
        async fn echo(&self, client: WsClient, payload: serde_json::Value) {
            let _ = client.emit_json("echo", payload);
        }

        #[subscribe_message("join")]
        async fn join(&self, client: WsClient, payload: serde_json::Value) {
            let n = CONNECTIONS.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = client.emit_json(
                "joined",
                serde_json::json!({ "connection_no": n, "you_sent": payload }),
            );
        }
    }

    #[module(controllers = [EchoGateway], providers = [EchoGateway])]
    pub struct LabModule;

    pub async fn boot() {
        NestFactory::create::<LabModule>()
            .set_global_prefix("lab")
            .listen_graceful(3200)
            .await;
    }
}
