//! Lab 2 — real-time: WebSocket gateway (echo + join counter) and an SSE stream.
//!
//! Run: `cargo run -p lab --bin lab2_realtime` then connect with a WS client / curl.

use nestrs::prelude::*;
use nestrs::ws::WsClient;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static CONNECTIONS: AtomicU64 = AtomicU64::new(0);

fn tick_stream() -> impl futures_util::Stream<Item = Result<nestrs::sse::Event, std::io::Error>> {
    futures_util::stream::unfold(0u32, |n| async move {
        if n >= 5 {
            return None; // stream completes after 5 ticks
        }
        let ev = nestrs::sse::Event::default()
            .event("tick")
            .data(format!("{{ \"n\": {n} }}"));
        Some((Ok(ev), n + 1))
    })
}

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

#[controller(prefix = "/rt")]
pub struct RtController;

#[routes(state = EchoGateway)]
impl RtController {
    #[get("/health")]
    pub async fn health(State(_gw): State<Arc<EchoGateway>>) -> &'static str {
        "realtime-ok"
    }

    #[get("/ticks")]
    pub async fn ticks()
    -> nestrs::sse::Sse<impl futures_util::Stream<Item = Result<nestrs::sse::Event, std::io::Error>>>
    {
        nestrs::sse::Sse::new(tick_stream()).keep_alive(nestrs::sse::KeepAlive::default())
    }
}

#[module(controllers = [RtController, EchoGateway], providers = [EchoGateway])]
pub struct LabModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<LabModule>()
        .set_global_prefix("lab")
        .listen_graceful(3200)
        .await;
}
