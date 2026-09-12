//! Tests for the runtime pipe chain — verifies the new
//! `PipedBody*` / `PipedQuery*` / `PipedPath*` extractors (added in
//! Wave 5.1) execute the pipe chain at extraction time, returning
//! `HttpException` (with the original status code) on failure.
//!
//! The `#[use_pipes(ValidationPipe)]` fast path is regression-tested in
//! `param_decorators_and_pipes.rs` and is preserved bit-for-bit.
//!
//! # Macro vs manual usage
//!
//! The `#[use_pipes(P)]` macro today only auto-rewrites parameters when
//! the chain contains `ValidationPipe` (the legacy fast path). For
//! chains with other pipes, write the extractor explicitly — these tests
//! exercise the extractors directly via `axum::extract::FromRequestParts`
//! without going through `#[routes]`.
//!
//! `#[param::req]` / `#[param::headers]` / `#[param::ip]` extractors
//! stay on their raw axum types even with `#[use_pipes]` present (the
//! `param_decorators_and_pipes` suite covers that).
//!
//! We use a manual `must` helper rather than `.expect()` because the
//! `Piped*` extractor types don't derive `Debug` (their `Output` type is
//! a projection through `PipeTransform`, which would force every pipe
//! type to be `Debug`-able — too restrictive).

use axum::body::Body;
use axum::extract::{FromRequest, Request};
use axum::http::{header, Request as HttpRequest, StatusCode};
use axum::routing::get;
use axum::Router;
use nestrs::prelude::*;
use tower::ServiceExt;

/// Pull a `Result<T, HttpException>` into a `T`, panicking with the
/// exception's status + message on failure. Avoids requiring `T: Debug`.
fn must<T>(r: Result<T, HttpException>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("extract failed: status={} message={}", e.status, e.message),
    }
}

#[tokio::test]
async fn parse_int_pipe_coerces_valid_path_segment_to_i64() {
    // ParseIntPipe's intended use is on a single path segment
    // (`/users/:id` → "42" → 42). We dispatch through a real axum router
    // (with the `:id` route matcher) so `MatchedPath` is set — that's the
    // only way `axum::extract::Path<T>` populates from the request.
    async fn handler(raw: PipedPath1<String, ParseIntPipe>) -> Result<String, HttpException> {
        Ok(raw.0.to_string())
    }

    let app = Router::new().route("/:id", get(handler));

    let resp = app
        .oneshot(
            HttpRequest::builder()
                .uri("/42")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("dispatch");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 1024)
        .await
        .expect("read body");
    assert_eq!(&body[..], b"42");
}

#[tokio::test]
async fn parse_int_pipe_returns_400_on_invalid_path_segment() {
    async fn handler(_raw: PipedPath1<String, ParseIntPipe>) -> Result<String, HttpException> {
        Ok("unreachable".to_string())
    }

    let app = Router::new().route("/:id", get(handler));

    let resp = app
        .oneshot(
            HttpRequest::builder()
                .uri("/abc")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("dispatch");

    // ParseIntPipe returns BadRequestException (400). Confirms the pipe error
    // propagates through the rejection → IntoResponse path without being
    // silently swallowed or rewritten.
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn trim_pipe_strips_leading_and_trailing_whitespace_from_body_string() {
    let req = Request::builder()
        .method("POST")
        .uri("/")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(r#""   hello   ""#))
        .expect("request");

    let extractor =
        must(<PipedBody1<String, TrimPipe> as FromRequest<()>>::from_request(req, &()).await);

    assert_eq!(extractor.0, "hello");
}

#[tokio::test]
async fn trim_pipe_preserves_internal_whitespace() {
    let req = Request::builder()
        .method("POST")
        .uri("/")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(r#""  hello world  ""#))
        .expect("request");

    let extractor =
        must(<PipedBody1<String, TrimPipe> as FromRequest<()>>::from_request(req, &()).await);

    assert_eq!(extractor.0, "hello world");
}
