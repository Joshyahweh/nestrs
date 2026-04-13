//! ## Adapter model
//!
//! | Nest                         | nestrs default                                      |
//! |-----------------------------|-----------------------------------------------------|
//! | Platform `ws` / Socket.IO   | Axum [`WebSocketUpgrade`](axum::extract::ws::WebSocketUpgrade) + JSON events |
//! | Multiple adapter packages   | One framing (`[`crate::WsEvent`]`) — swap by implementing [`crate::WsGateway`] yourself |
//!
//! For **Socket.IO**, use a dedicated crate (for example **socketioxide**) and merge its router; nestrs does not embed a second protocol stack.
