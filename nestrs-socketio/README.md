# nestrs-socketio

Socket.IO adapter for nestrs. NestJS ships `@nestjs/platform-socket.io`;
nestrs-ws is RFC 6455 only. This crate plugs [socketioxide](https://crates.io/crates/socketioxide)
into the Axum router `NestApplication` already uses.

```toml
nestrs-socketio = "1.4.0"
```

```rust,ignore
use nestrs_socketio::{merge_socketio, socketio_layer};

let (layer, io) = socketio_layer();
io.ns("/", |s| async move { /* s.on("message", ...) */ });
let router = merge_socketio(app.into_router(), layer);
```
