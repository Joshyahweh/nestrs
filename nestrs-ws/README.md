# nestrs-ws

**WebSocket gateway** helpers for [nestrs](https://crates.io/crates/nestrs): JSON frames shaped as `{ "event": "...", "data": ... }`, **`WsClient`** for sending back to a connection, and **`ws_route`** to plug an Axum `GET` upgrade handler.

Macros in **`nestrs`** (`#[ws_routes]`, `#[subscribe_message]`) generate `WsGateway` implementations on top of this crate.

**Upgrade context:** `ws_route` captures **`HeaderMap`** into **`WsHandshake`** on each **`WsClient`** (`handshake()` / `handshake.headers()`) for auth and routing.

**Cross-cutting (Nest-like, not identical):** on the gateway struct, use **`#[use_ws_guards(Type, ...)]`**, **`#[use_ws_pipes(Type, ...)]`**, **`#[use_ws_interceptors(Type, ...)]`** — implemented types implement **`WsCanActivate`**, **`WsPipeTransform`**, **`WsIncomingInterceptor`** from this crate. Order: interceptors → guards → pipes → handler.

**Errors vs HTTP exception filters:** WebSocket frames are not Axum HTTP responses — **`nestrs::NestApplication::use_global_exception_filter` does not run** on WS JSON. Server-side failures are emitted on the event **`WS_ERROR_EVENT`** (`"error"`). Shapes are documented in the **crate-level rustdoc** on [`nestrs-ws`](https://docs.rs/nestrs-ws) (guards, pipes, unknown events, bad DTO parse, wire parse). Centralize behavior with shared guard/pipe types or a thin `WsGateway` wrapper.

**Adapters:** see **`nestrs_ws::adapters`** — Axum uses RFC 6455 **`WebSocket`**; for **Socket.IO**-style clients use **[`socketioxide`](https://crates.io/crates/socketioxide)** or keep the JSON event contract documented here.

**Docs:** [docs.rs/nestrs-ws](https://docs.rs/nestrs-ws) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-ws = "0.2.0"
axum = { version = "0.7", features = ["ws"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Or enable the feature on the umbrella crate:

```toml
nestrs = { version = "0.2.0", features = ["ws"] }
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
