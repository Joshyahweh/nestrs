//! Lab 2c — WS anomaly matrix: which frames fail?
//! Content suspect: `{"event":"join","data":{"room":"lab"}}`.
//! Position suspect: any second-frame-on-a-connection.
//!
//! Run: `cargo run -p lab --bin lab2_ws_matrix`

use futures_util::{SinkExt, StreamExt};
use nestrs::prelude::*;
use nestrs::ws::WsClient;
use tokio_tungstenite::tungstenite::Message;

#[ws_gateway(path = "/ws")]
#[derive(Default)]
#[injectable]
struct MatrixGateway;

#[ws_routes]
impl MatrixGateway {
    #[subscribe_message("echo")]
    async fn echo(&self, client: WsClient, payload: serde_json::Value) {
        let _ = client.emit_json("echo", payload);
    }

    #[subscribe_message("join")]
    async fn join(&self, client: WsClient, payload: serde_json::Value) {
        let _ = client.emit_json("joined", payload);
    }
}

#[module(controllers = [MatrixGateway], providers = [MatrixGateway])]
struct LabModule;

#[tokio::main]
async fn main() {
    tokio::spawn(async { boot().await });
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    // A: join{room} as FIRST frame on a fresh connection
    scenario(
        "A: join{room} first frame",
        vec![r#"{"event":"join","data":{"room":"lab"}}"#],
    )
    .await;

    // B: echo then join{room} (original failing order)
    scenario(
        "B: echo → join{room}",
        vec![
            r#"{"event":"echo","data":{"msg":"hi"}}"#,
            r#"{"event":"join","data":{"room":"lab"}}"#,
        ],
    )
    .await;

    // C: echo then join{} (is position 2 the problem?)
    scenario(
        "C: echo → join{}",
        vec![
            r#"{"event":"echo","data":{"msg":"hi"}}"#,
            r#"{"event":"join","data":{}}"#,
        ],
    )
    .await;

    println!("MATRIX DONE");
}

async fn scenario(label: &str, frames: Vec<&str>) {
    let (mut ws, _) = tokio_tungstenite::connect_async("ws://127.0.0.1:3300/ws")
        .await
        .expect("handshake");
    println!("--- {label} ---");
    for f in frames {
        ws.send(Message::text(f)).await.expect("send");
        match tokio::time::timeout(std::time::Duration::from_secs(1), ws.next()).await {
            Ok(Some(Ok(m))) => println!("  sent: {f}\n  got : {m}"),
            other => println!("  sent: {f}\n  got : {other:?}"),
        }
    }
    let _ = ws.close(None).await;
}

async fn boot() {
    NestFactory::create::<LabModule>()
        .listen_graceful(3300)
        .await;
}
