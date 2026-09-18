//! Socket.IO adapter for nestrs — NestJS `@nestjs/platform-socket.io` analogue.
//!
//! nestrs-ws speaks RFC 6455 JSON events. This crate mounts a real Socket.IO
//! stack ([`socketioxide`]) onto the same Axum router.

#![doc(html_root_url = "https://docs.rs/nestrs-socketio/1.3.0")]

use axum::Router;
use socketioxide::{layer::SocketIoLayer, SocketIo};

pub use socketioxide;

/// Build a Socket.IO Tower layer and the [`SocketIo`] handle used to register
/// namespaces (`io.ns("/", ...)`).
pub fn socketio_layer() -> (SocketIoLayer, SocketIo) {
    SocketIo::new_layer()
}

/// Merge the Socket.IO layer onto an Axum (nestrs) router.
pub fn merge_socketio(router: Router, layer: SocketIoLayer) -> Router {
    router.layer(layer)
}

#[cfg(test)]
mod tests {
    use super::socketio_layer;

    #[test]
    fn layer_builds() {
        let (_layer, io) = socketio_layer();
        let _ = io;
    }
}
