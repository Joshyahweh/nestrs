//! Streaming file responses (feature: **`files`**) — Axum body stream from disk.

use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use tokio::fs::File;
use tokio_util::io::ReaderStream;

/// Streams a file with the given `Content-Type` (no `Content-Length`; chunked transfer).
pub async fn stream_file_with_content_type(
    path: impl AsRef<std::path::Path>,
    content_type: &'static str,
) -> std::io::Result<Response> {
    let file = File::open(path.as_ref()).await?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);
    let ct = HeaderValue::from_static(content_type);
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, ct)
        .body(body)
        .unwrap())
}

/// Streams a file as `application/octet-stream`.
pub async fn stream_file_octet_stream(
    path: impl AsRef<std::path::Path>,
) -> std::io::Result<Response> {
    stream_file_with_content_type(path, "application/octet-stream").await
}

/// Maps `std::io::Error` into a plain **404** (missing file) or **500** response.
pub async fn stream_file_or_response(
    path: impl AsRef<std::path::Path>,
    content_type: &'static str,
) -> Response {
    match stream_file_with_content_type(path, content_type).await {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
