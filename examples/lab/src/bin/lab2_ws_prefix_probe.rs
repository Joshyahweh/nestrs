//! Lab 2d — discriminate WHY lab2_ws_client fails but lab2_ws_matrix passes:
//!   variable 1: global prefix (/lab/ws vs /ws)
//!   variable 2: gateway internals (atomic counter + enriched json)
//! Two servers, same failing sequence against each.
//!
//! Run: `cargo run -p lab --bin lab2_ws_prefix_probe`

use futures_util::{SinkExt, StreamExt};
use nestrs::prelude::*;
use nestrs::ws::{WsClient, WsGateway};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;

static CONNECTIONS: AtomicU64 = AtomicU64::new(0);

/// Identical to lab2_ws_client's EchoGateway (counter + enriched payload).
#[ws_gateway(path = "/ws")]
#[derive(Default)]
#[injectable]
struct EchoGateway;

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
struct LabModule;

#[tokio::main]
async fn main() {
    // Server A: WITH global prefix "lab" on 3500 (mirrors lab2_ws_client).
    tokio::spawn(async {
        NestFactory::create::<LabModule>()
            .set_global_prefix("lab")
            .listen_graceful(3500)
            .await;
    });
    // Server B: NO prefix on 3600 (mirrors lab2_ws_matrix).
    tokio::spawn(async {
        NestFactory::create::<LabModule>()
            .listen_graceful(3600)
            .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    println!("=== A: prefixed /lab/ws ===");
    run_sequence("ws://127.0.0.1:3500/lab/ws").await;
    println!("=== B: unprefixed /ws ===");
    run_sequence("ws://127.0.0.1:3600/ws").await;

    println!("PROBE DONE");
}

async fn run_sequence(url: &str) {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.expect("handshake");
    ws.send(Message::text(
        r#"{"event":"echo","data":{"msg":"hello over ws"}}"#,
    ))
    .await
    .expect("send echo");
    let r = ws.next().await.expect("reply").expect("ok");
    println!("  msg1 reply: {r}");
    ws.send(Message::text(r#"{"event":"join","data":{"room":"lab"}}"#))
        .await
        .expect("send join");
    let r = ws.next().await.expect("reply").expect("ok");
    println!("  msg2 reply: {r}");
    let _ = ws.close(None).await;
}
