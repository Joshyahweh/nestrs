//! Third-party and in-house transporters (Nest “custom transport” analogue).
//!
//! ## Client
//!
//! Implement [`crate::Transport`] (`send_json` / `emit_json`) for your broker SDK, then register
//! the client with [`crate::ClientConfig::new`].
//!
//! ## Server / consumer
//!
//! Use the same JSON payloads as the built-in adapters:
//!
//! - Deserialize the body as [`crate::wire::WireRequest`].
//! - For `WireKind::Send`, call [`crate::wire::dispatch_send`] with your handler stack and publish
//!   the resulting [`crate::wire::WireResponse`] JSON to the queue or subject named in
//!   `WireRequest::reply` (when set).
//! - For `WireKind::Emit`, call [`crate::wire::dispatch_emit`] (fire-and-forget).
//!
//! See `tcp.rs`, `redis.rs`, and `rabbitmq/live.rs` in this crate for reference loops.
