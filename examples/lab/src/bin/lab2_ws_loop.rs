//! Lab 2e — hammer the suspicious sequence N times to measure the byte-loss rate.
//! Each iteration: fresh connection → echo → read → join{room} → read.
//!
//! Run: `cargo run -p lab --bin lab2_ws_loop`

use futures_util::{SinkExt, StreamExt};
use nestrs::prelude::*;
use nestrs::ws::{WsClient, WsGateway};
use tokio_tungstenite::tungstenite::Message;

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
        let _ = client.emit_json("joined", payload);
    }
}

#[module(controllers = [EchoGateway], providers = [EchoGateway])]
struct LabModule;

const JOIN_FRAME: &str = r#"{"event":"join","data":{"room":"lab"}}"#;
const ECHO_FRAME: &str = r#"{"event":"echo","data":{"msg":"hello over ws"}}"#;
const ITERATIONS: usize = 100;

#[tokio::main]
async fn main() {
    tokio::spawn(async {
        NestFactory::create::<LabModule>()
            .set_global_prefix("lab")
            .listen_graceful(3700)
            .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let mut failures = 0usize;
    let mut first_failure: Option<String> = None;
    for i in 0..ITERATIONS {
        let (mut ws, _) =
            tokio_tungstenite::connect_async("ws://127.0.0.1:3700/lab/ws").await.expect("hs");
        ws.send(Message::text(ECHO_FRAME)).await.expect("send");
        let r1 = ws.next().await.expect("reply").expect("ok");
        assert!(r1.to_text().unwrap().contains("echo"), "iter {i}: echo broken");

        ws.send(Message::text(JOIN_FRAME)).await.expect("send");
        let r2 = tokio::time::timeout(std::time::Duration::from_secs(1), ws.next()).await;
        match r2 {
            Ok(Some(Ok(m))) => {
                let t = m.into_text().unwrap_or_default();
                if !t.contains("joined") {
                    failures += 1;
                    if first_failure.is_none() {
                        first_failure = Some(format!("iter {i}: expected joined, got: {t}"));
                    }
                }
            }
            other => {
                failures += 1;
                if first_failure.is_none() {
                    first_failure = Some(format!("iter {i}: no reply: {other:?}"));
                }
            }
        }
        let _ = ws.close(None).await;
    }
    println!("{failures}/{ITERATIONS} iterations lost/corrupted the join frame");
    if let Some(f) = first_failure {
        println!("first failure: {f}");
    }
}
