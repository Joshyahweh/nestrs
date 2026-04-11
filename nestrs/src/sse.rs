/// Server-Sent Events helpers (NestJS `@Sse()` analogue).
///
/// This module re-exports Axum's SSE response types so applications can use SSE without adding a
/// direct `axum` dependency.
pub use axum::response::sse::{Event, KeepAlive, Sse};
