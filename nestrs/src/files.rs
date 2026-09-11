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

/// Whether `name` is safe to `join` onto a base directory: exactly one
/// path component — no separators (`/` or `\`, so percent-decoded
/// `%2F`/`%5C` are caught after extraction), no traversal segments
/// (`.`/`..`), no NUL. On Windows, `:` is also rejected (drive letters and
/// NTFS alternate data streams).
fn is_safe_path_component(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
        && !(cfg!(windows) && name.contains(':'))
}

/// Streams `base/<name>` where `name` comes from an **untrusted path
/// parameter** — the safe shape for `#[get("/download/:name")]` handlers:
///
/// ```ignore
/// #[get("/download/:name")]
/// pub async fn download(#[param::param] p: NameParams) -> Response {
///     nestrs::stream_file_from_dir(upload_dir(), &p.name, "application/octet-stream").await
/// }
/// ```
///
/// `name` must be a single path component (see [`is_safe_path_component`]
/// semantics): anything containing separators or traversal segments —
/// including values that arrive percent-encoded (`..%2F..%2Fetc%2Fpasswd`
/// decodes before the handler sees it) — is rejected with **400**, never
/// `join`ed. The resolved path is additionally canonicalized and required
/// to stay inside `base`, so a symlink planted inside the directory
/// pointing outside is reported as **404** rather than followed. Missing
/// files are **404**; other I/O errors **500**.
///
/// For fully trusted paths (not derived from request input),
/// [`stream_file_or_response`] is the direct form.
pub async fn stream_file_from_dir(
    base: impl AsRef<std::path::Path>,
    name: impl AsRef<str>,
    content_type: &'static str,
) -> Response {
    let name = name.as_ref();
    if !is_safe_path_component(name) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let base = base.as_ref();
    let file_path = base.join(name);
    // Component validation blocks direct traversal; canonicalization also
    // blocks symlink plants inside `base` pointing outside it.
    let real = match tokio::fs::canonicalize(&file_path).await {
        Ok(real) => real,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return StatusCode::NOT_FOUND.into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let real_base = match tokio::fs::canonicalize(base).await {
        Ok(real_base) => real_base,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    if !real.starts_with(&real_base) {
        // Symlink escape — treat as missing rather than disclosing.
        return StatusCode::NOT_FOUND.into_response();
    }
    stream_file_or_response(&real, content_type).await
}

/// Maps `std::io::Error` into a plain **404** (missing file) or **500** response.
///
/// The ergonomic choice inside handlers — returns a `Response` directly instead of `Result`:
///
/// ```ignore
/// #[get("/download/:name")]
/// pub async fn download(#[param::param] p: NameParams) -> Response {
///     nestrs::stream_file_from_dir(upload_dir(), &p.name, "application/octet-stream").await
/// }
/// ```
///
/// For paths built from request input (path/query parameters), prefer
/// [`stream_file_from_dir`], which rejects traversal before joining.
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
