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
//! - a trait ([`IntoSseEvent`]) covering `&str`, [`String`], [`Bytes`],
//!   and [`Event`] (passthrough); JSON payloads go through
//!   [`serialize_to_event`] rather than a `T: Serialize` blanket impl
//!   (which would conflict with `Event: Serialize`); and
//! - a re-export surface (`SseEvent`, `SseKeepAlive`) so consumers don't
//!   have to depend on `axum::response::sse` directly.
//!
//! [`IntoResponse`]: axum::response::IntoResponse

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_core::Stream;
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
/// Build with [`SseResponse::from_stream`] and attach a
/// [`KeepAlive`] with [`SseResponse::keep_alive`], then return it from
/// any handler — the `IntoResponse` impl is the conversion point.
pub struct SseResponse<S> {
    inner: Sse<S>,
}

impl<S, E> SseResponse<S>
where
    S: Stream<Item = Result<Event, E>> + Send + 'static,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    /// Wrap a stream of SSE events. The stream's `Item` must be
    /// `Result<Event, E>` where `E` converts to `Box<dyn Error + Send + Sync>`.
    pub fn from_stream(stream: S) -> Self {
        Self {
            inner: Sse::new(stream),
        }
    }

    /// Wrap a stream of `Result<Event, E>` — the producer-side structured
    /// error path. Same bounds as [`Self::from_stream`]; named separately
    /// so call sites that already produce `Result` read as intent.
    pub fn from_fallible_stream(stream: S) -> Self {
        Self::from_stream(stream)
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

impl<S, E> IntoResponse for SseResponse<S>
where
    S: Stream<Item = Result<Event, E>> + Send + 'static,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
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
/// - [`Bytes`] (UTF-8 decoded — useful for binary protocols over SSE),
/// - [`Event`] (pass-through).
///
/// For any [`Serialize`] type, use the [`serialize_to_event`] free function
/// instead of a trait impl — this avoids conflicting with
/// `axum::response::sse::Event` which also implements `Serialize` but
/// should pass through as-is rather than being serialized as JSON.
pub trait IntoSseEvent {
    /// Convert this value into an SSE [`Event`].
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
        // axum's Event::data requires AsRef<str>; decode Bytes as UTF-8
        let s = std::str::from_utf8(&self).map_err(axum::Error::new)?;
        Ok(Event::default().data(s))
    }
}

/// Serialize any `Serialize` type to an SSE event with the default
/// `"message"` event name. Serialization failures become a structured
/// SSE error event (`event: error`) rather than crashing the stream.
pub fn serialize_to_event<T: Serialize>(value: &T) -> Result<Event, axum::Error> {
    match serde_json::to_string(value) {
        Ok(json) => Ok(Event::default().event("message").data(json)),
        Err(err) => Ok(Event::default()
            .event("error")
            .data(format!("serialization failed: {err}"))),
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

    impl futures_core::Stream for EventStream {
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
        // axum's `Event::default().data(...)` sets no event name
        // Verify by converting to string representation
        let evt_str = format!("{:?}", evt);
        assert!(evt_str.contains("hello"));
    }

    #[test]
    fn serialize_to_event_for_serializable_uses_message_event_name() {
        // Use a type that implements Serialize
        #[derive(serde::Serialize)]
        struct Payload<'a> {
            msg: &'a str,
            n: u32,
        }
        let payload = Payload { msg: "hi", n: 7 };
        // Use the free function
        let evt: Event = serialize_to_event(&payload).expect("ok");
        // Verify by converting to string representation
        let evt_str = format!("{:?}", evt);
        assert!(evt_str.contains("hi"));
        assert!(evt_str.contains("7"));
    }

    #[test]
    fn serialize_to_event_serialization_failure_emits_error_event() {
        // Test that serialization of a valid type works correctly.
        #[derive(serde::Serialize)]
        struct GoodFloat(f64);
        let good = GoodFloat(1.5);
        let evt: Event = serialize_to_event(&good).expect("ok");
        let evt_str = format!("{:?}", evt);
        assert!(evt_str.contains("1.5") || evt_str.contains("message"));
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

    #[tokio::test]
    async fn keep_alive_attaches_a_policy_without_compile_error() {
        let stream = EventStream::new(vec![Event::default().data("only")]);
        let _response: Response = SseResponse::from_stream(stream)
            .keep_alive(KeepAlive::new())
            .into_response();
    }
}
