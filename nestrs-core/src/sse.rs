//! Server-Sent Events (SSE) response wrapper — `SseResponse<S>`.
//!
//! This module re-exports axum's SSE primitives behind a stable surface so
//! handlers can return a unified [`SseResponse<S>`] regardless of which crate
//! (`axum::response::sse`, `nestrs_http`, `nestrs`) is wiring the response.
//! The intent mirrors the `nestrs_oauth2::cookies` ergonomics: a thin
//! nestrs-flavored wrapper around axum that adds
//!
//! - a single conversion point for `Sse<S>` → `axum::response::Response`
//!   ([`SseResponse`] implements [`IntoResponse`] so any handler can return
//!   it without naming axum's type);
//! - a trait ([`IntoSseEvent`]) covering the common payload shapes
//!   (`&str`, `String`, `Bytes`, anything `Serialize`); and
//! - a re-export surface (`SseEvent`, `SseKeepAlive`) so consumers don't
//!   have to depend on `axum::response::sse` directly.
//!
//! [`IntoResponse`]: axum::response::IntoResponse

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use serde::Serialize;

/// An SSE event. Re-export of [`axum::response::sse::Event`] so users
/// don't need a direct axum dep to construct one.
pub type SseEvent = Event;

/// A keep-alive policy for SSE streams. Re-export of
/// [`axum::response::sse::KeepAlive`].
pub type SseKeepAlive = KeepAlive;

/// A nestrs-flavored wrapper around [`Sse<S>`] that converts to an
/// [`axum::response::Response`] without forcing handlers to name axum's
/// SSE type directly.
///
/// Build with [`SseResponse::from_stream`] (infallible) or
/// [`SseResponse::from_fallible_stream`] (when each event can fail with an
/// [`axum::Error`]), attach a [`KeepAlive`] with [`SseResponse::keep_alive`],
/// and return it from any handler — the `IntoResponse` impl is the
/// conversion point.
pub struct SseResponse<S> {
    inner: Sse<S>,
}

impl<S> SseResponse<S> {
    /// Wrap a stream of infallible SSE events. The stream's `Item` must
    /// implement [`IntoSseEvent`].
    pub fn from_stream(stream: S) -> Self {
        Self {
            inner: Sse::new(stream),
        }
    }

    /// Unwrap into the underlying [`Sse<S>`]. Use this when you need to
    /// reach for an axum-only API the wrapper doesn't expose.
    pub fn into_inner(self) -> Sse<S> {
        self.inner
    }

    /// Borrow the inner [`Sse<S>`] without consuming the wrapper.
    pub fn as_inner(&self) -> &Sse<S> {
        &self.inner
    }
}

impl<S> SseResponse<S>
where
    S: futures_core::Stream<Item = Result<Event, axum::Error>>,
{
    /// Wrap a stream of fallible SSE events. Each item is a
    /// `Result<Event, axum::Error>` — typically used when the producer
    /// wants a structured error path (network drop, serialization
    /// failure) instead of crashing the stream.
    pub fn from_fallible_stream(stream: S) -> Self {
        Self {
            inner: Sse::new(stream),
        }
    }
}

impl<S> SseResponse<S> {
    /// Attach a [`KeepAlive`] policy (interval, text, etc.) to the
    /// response. Mirrors [`Sse::keep_alive`].
    pub fn keep_alive(self, keep_alive: KeepAlive) -> Self {
        Self {
            inner: self.inner.keep_alive(keep_alive),
        }
    }
}

impl<S> From<Sse<S>> for SseResponse<S> {
    fn from(inner: Sse<S>) -> Self {
        Self { inner }
    }
}

impl<S> IntoResponse for SseResponse<S>
where
    S: futures_core::Stream<Item = Result<Event, axum::Error>> + Send + 'static,
{
    fn into_response(self) -> Response {
        // Delegate to axum's own conversion so behavior (headers, chunked
        // transfer encoding, retry semantics) stays identical.
        self.inner.into_response()
    }
}

/// Trait for values that can become an SSE event. Implemented for the
/// payload shapes most handler authors reach for:
///
/// - [`&str`] and [`String`] (rendered as the default `message` event),
/// - [`Bytes`] (rendered raw — useful for binary protocols over SSE),
/// - any `T: Serialize` (auto-JSON-encoded, event name `message`).
///
/// Serialization failures are converted into a structured SSE error event
/// (`event: error`) rather than crashing the stream — callers should still
/// log the failure on the producer side if they care to surface it.
pub trait IntoSseEvent {
    fn into_sse_event(self) -> Result<Event, axum::Error>;
}

impl IntoSseEvent for Event {
    fn into_sse_event(self) -> Result<Event, axum::Error> {
        Ok(self)
    }
}

impl IntoSseEvent for &str {
    fn into_sse_event(self) -> Result<Event, axum::Error> {
        Ok(Event::default().data(self))
    }
}

impl IntoSseEvent for String {
    fn into_sse_event(self) -> Result<Event, axum::Error> {
        Ok(Event::default().data(self))
    }
}

impl IntoSseEvent for Bytes {
    fn into_sse_event(self) -> Result<Event, axum::Error> {
        Ok(Event::default().data(self))
    }
}

impl<T> IntoSseEvent for T
where
    T: Serialize,
{
    fn into_sse_event(self) -> Result<Event, axum::Error> {
        match serde_json::to_string(&self) {
            Ok(json) => Ok(Event::default().event("message").data(json)),
            Err(err) => Ok(Event::default()
                .event("error")
                .data(format!("serialization failed: {err}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll};

    /// Minimal in-memory stream of pre-built events. Used by unit tests
    /// so we don't pull `futures-util` into the module's dev-dep set.
    struct EventStream {
        events: Arc<Vec<Event>>,
        idx: usize,
    }

    impl EventStream {
        fn new(events: Vec<Event>) -> Self {
            Self {
                events: Arc::new(events),
                idx: 0,
            }
        }
    }

    impl Stream for EventStream {
        type Item = Result<Event, axum::Error>;

        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let this = self.get_mut();
            if this.idx >= this.events.len() {
                Poll::Ready(None)
            } else {
                let evt = this.events[this.idx].clone();
                this.idx += 1;
                Poll::Ready(Some(Ok(evt)))
            }
        }
    }

    #[test]
    fn into_sse_event_for_str_uses_default_message_event() {
        let evt: Event = "hello".into_sse_event().expect("ok");
        // axum's `Event::default().data(...)` sets no event name, so the
        // serialized payload begins with `data: hello\n\n`.
        assert_eq!(evt.event.as_deref(), None);
    }

    #[test]
    fn into_sse_event_for_serializable_uses_message_event_name() {
        #[derive(Serialize)]
        struct Payload<'a> {
            msg: &'a str,
            n: u32,
        }
        let evt: Event = Payload { msg: "hi", n: 7 }
            .into_sse_event()
            .expect("ok");
        assert_eq!(evt.event.as_deref(), Some("message"));
    }

    #[test]
    fn into_sse_event_serialization_failure_emits_error_event() {
        // `serde_json` rejects `f64::NAN` even though Serialize accepts it.
        let bad: f64 = f64::NAN;
        let evt: Event = bad.into_sse_event().expect("event emitted");
        assert_eq!(evt.event.as_deref(), Some("error"));
    }

    #[test]
    fn into_response_emits_text_event_stream_content_type() {
        let stream = EventStream::new(vec![
            Event::default().data("alpha"),
            Event::default().data("beta"),
            Event::default().event("end").data("done"),
        ]);
        let response: Response = SseResponse::from_stream(stream).into_response();
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .expect("content-type set"),
            "text/event-stream",
        );
    }

    #[test]
    fn keep_alive_attaches_a_policy_without_compile_error() {
        let stream = EventStream::new(vec![Event::default().data("only")]);
        let _response: Response = SseResponse::from_stream(stream)
            .keep_alive(KeepAlive::new())
            .into_response();
    }
}
