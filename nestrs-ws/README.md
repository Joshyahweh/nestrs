# nestrs-ws

**WebSocket gateway** helpers for [nestrs](https://crates.io/crates/nestrs): JSON frames shaped as `{ "event": "...", "data": ... }`, **`WsClient`** for sending back to a connection, and **`ws_route`** to plug an Axum `GET` upgrade handler.

Macros in **`nestrs`** (`#[ws_routes]`, `#[subscribe_message]`) generate `WsGateway` implementations on top of this crate.

**Docs:** [docs.rs/nestrs-ws](https://docs.rs/nestrs-ws) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-ws = "0.1.3"
axum = { version = "0.7", features = ["ws"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Or enable the feature on the umbrella crate:

```toml
nestrs = { version = "0.1.3", features = ["ws"] }
```

## Example: manual `WsGateway`

```rust
use async_trait::async_trait;
use nestrs_ws::{ws_route, WsClient, WsGateway};
use std::sync::Arc;

struct ChatGateway;

#[async_trait]
impl WsGateway for ChatGateway {
    async fn on_message(&self, client: WsClient, event: &str, payload: serde_json::Value) {
        if event == "ping" {
            let _ = client.emit("pong", payload);
        }
    }
}

fn main() {
    let gateway = Arc::new(ChatGateway);
    let _route = ws_route(gateway);
    // Merge `_route` into your Axum Router at a path, e.g. `.route("/ws", _route)`.
}
```

## License

MIT OR Apache-2.0.
