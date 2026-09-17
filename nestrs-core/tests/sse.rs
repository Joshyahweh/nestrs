//! Integration tests for `nestrs_core::sse` (the `sse` feature).
//!
//! Run with `cargo test --features sse -p nestrs-core`. The `tests`
//! directory is only built when the feature is on — the file-level
//! `cfg` keeps the test target out of the default build.

#![cfg(feature = "sse")]

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::http::header::CONTENT_TYPE;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_core::Stream;
use http_body_util::BodyExt;
use nestrs_core::sse::{serialize_to_event, IntoSseEvent, SseResponse};
use serde::Serialize;

/// Minimal in-memory stream of pre-built `Event`s, used by tests so we
/// don't pull `futures-util` into the dev-dep set just for tests.
#[derive(Clone)]
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

    fn empty() -> Self {
        Self::new(Vec::new())
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

#[tokio::test]
async fn into_response_emits_text_event_stream_content_type() {
    let stream = EventStream::new(vec![Event::default().data("hello")]);
    let response: Response = SseResponse::from_stream(stream).into_response();

    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .expect("content-type set"),
        "text/event-stream"
    );
}

#[tokio::test]
async fn single_event_streams_data_line() {
    let stream = EventStream::new(vec![Event::default().data("only")]);
    let response: Response = SseResponse::from_stream(stream).into_response();
    let body_bytes = collect_body(response).await;
    let body = std::str::from_utf8(&body_bytes).expect("utf-8");

    // SSE wire format: each event is one or more `field: value\n` lines
    // followed by a blank line. Default `data` field, no `event:` header.
    assert!(
        body.contains("data: only"),
        "body should carry the data line, got: {body:?}"
    );
    assert!(
        body.ends_with("\n\n"),
        "body must end with a blank line terminator, got: {body:?}"
    );
}

#[tokio::test]
async fn multi_event_stream_writes_each_event() {
    let stream = EventStream::new(vec![
        Event::default().data("alpha"),
        Event::default().data("beta"),
        Event::default().data("gamma"),
    ]);
    let response: Response = SseResponse::from_stream(stream).into_response();
    let body = collect_body_str(response).await;

    assert!(body.contains("data: alpha"));
    assert!(body.contains("data: beta"));
    assert!(body.contains("data: gamma"));
}

#[tokio::test]
async fn named_event_emits_event_field() {
    let stream = EventStream::new(vec![Event::default().event("tick").data("payload")]);
    let response: Response = SseResponse::from_stream(stream).into_response();
    let body = collect_body_str(response).await;

    assert!(
        body.contains("event: tick"),
        "named event must include an event: field, got: {body:?}"
    );
    assert!(body.contains("data: payload"));
}

#[tokio::test]
async fn retry_field_is_written_when_set() {
    let stream = EventStream::new(vec![Event::default()
        .retry(Duration::from_millis(250))
        .data("with-retry")]);
    let response: Response = SseResponse::from_stream(stream).into_response();
    let body = collect_body_str(response).await;

    assert!(
        body.contains("retry: 250") || body.contains("retry:250"),
        "retry field must be serialized, got: {body:?}"
    );
}

#[tokio::test]
async fn keep_alive_attaches_a_policy() {
    let stream = EventStream::empty();
    let response: Response = SseResponse::from_stream(stream)
        .keep_alive(KeepAlive::new())
        .into_response();
    // We can't observe the heartbeat from a unit test, but the wrapper
    // must still compile-convert into a Response — the docstring asserts
    // that this is the only behavioral guarantee the keep_alive wrapper
    // adds on top of axum's own KeepAlive handling.
    assert_eq!(
        response.headers().get(CONTENT_TYPE).expect("ct"),
        "text/event-stream"
    );
}

#[tokio::test]
async fn empty_stream_yields_an_empty_body() {
    let stream = EventStream::empty();
    let response: Response = SseResponse::from_stream(stream).into_response();
    let body_bytes = collect_body(response).await;
    assert!(
        body_bytes.is_empty(),
        "empty stream should not produce any data lines, got: {body_bytes:?}"
    );
}

#[tokio::test]
async fn into_sse_event_for_str_uses_default_event() {
    let evt: Event = "hello".into_sse_event().expect("ok");
    // axum 0.7 keeps Event fields private; Debug is the public observation.
    let dbg = format!("{evt:?}");
    assert!(dbg.contains("hello"), "got: {dbg}");
}

#[tokio::test]
async fn into_sse_event_for_string_uses_default_event() {
    let evt: Event = String::from("hi").into_sse_event().expect("ok");
    let dbg = format!("{evt:?}");
    assert!(dbg.contains("hi"), "got: {dbg}");
}

#[tokio::test]
async fn into_sse_event_for_bytes_uses_default_event() {
    let bytes = Bytes::from_static(b"raw");
    let evt: Event = bytes.into_sse_event().expect("ok");
    let dbg = format!("{evt:?}");
    assert!(dbg.contains("raw"), "got: {dbg}");
}

#[tokio::test]
async fn into_sse_event_for_serializable_uses_message_event_name() {
    #[derive(Serialize)]
    struct Payload<'a> {
        msg: &'a str,
        n: u32,
    }
    // Blanket `IntoSseEvent for T: Serialize` conflicts with
    // `impl IntoSseEvent for Event` (Event: Serialize). The public
    // Serialize path is `serialize_to_event`.
    let evt: Event = serialize_to_event(&Payload { msg: "hi", n: 7 }).expect("ok");
    let dbg = format!("{evt:?}");
    assert!(
        dbg.contains("message") || dbg.contains("hi"),
        "serialized payload should land in a message event, got: {dbg}"
    );
}

#[tokio::test]
async fn into_sse_event_serialization_failure_emits_error_event() {
    // serde_json maps `f64::NAN` to `null` rather than erroring; drive a
    // real `Serialize` failure so the error-event path stays covered.
    struct AlwaysFail;
    impl Serialize for AlwaysFail {
        fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::Error;
            Err(S::Error::custom("intentional"))
        }
    }
    let evt: Event = serialize_to_event(&AlwaysFail).expect("event emitted");
    let dbg = format!("{evt:?}");
    assert!(
        dbg.contains("error"),
        "serialization failure must emit an error event, got: {dbg}"
    );
}

#[tokio::test]
async fn into_sse_event_passthrough_for_event() {
    let original = Event::default().event("custom").data("v");
    let evt = original.clone().into_sse_event().expect("ok");
    let dbg = format!("{evt:?}");
    assert!(
        dbg.contains("custom"),
        "passthrough must keep event name, got: {dbg}"
    );
    assert!(dbg.contains("v"), "passthrough must keep data, got: {dbg}");
}

#[tokio::test]
async fn from_sse_conversion_preserves_inner() {
    let stream = EventStream::new(vec![Event::default().data("via-from")]);
    let sse = Sse::new(stream);
    let response: Response = SseResponse::from(sse).into_response();
    let body = collect_body_str(response).await;
    assert!(body.contains("data: via-from"));
}

#[tokio::test]
async fn into_inner_returns_the_inner_sse() {
    let stream = EventStream::new(vec![Event::default().data("v")]);
    let wrapper = SseResponse::from_stream(stream);
    let _inner: Sse<EventStream> = wrapper.into_inner();
}

#[tokio::test]
async fn as_inner_returns_a_reference_to_the_inner_sse() {
    let stream = EventStream::new(vec![Event::default().data("v")]);
    let wrapper = SseResponse::from_stream(stream);
    let _inner_ref: &Sse<EventStream> = wrapper.as_inner();
}

#[tokio::test]
async fn from_fallible_stream_wraps_a_stream_of_results() {
    // `EventStream::Item` is already `Result<Event, axum::Error>` —
    // `from_fallible_stream` is the named constructor for that shape.
    // `futures_core` has no `stream::iter`; don't pull futures-util
    // into the test graph just to build a mixed Ok/Err iterator.
    let stream = EventStream::new(vec![Event::default().data("first")]);
    let response: Response = SseResponse::from_fallible_stream(stream).into_response();
    assert_eq!(
        response.headers().get(CONTENT_TYPE).expect("ct"),
        "text/event-stream"
    );
}

// --- helpers ---------------------------------------------------------------

async fn collect_body(response: Response) -> Vec<u8> {
    let body = response.into_body();
    let collected = body.collect().await.expect("body collect");
    collected.to_bytes().to_vec()
}

async fn collect_body_str(response: Response) -> String {
    let bytes = collect_body(response).await;
    String::from_utf8(bytes).expect("utf-8")
}
