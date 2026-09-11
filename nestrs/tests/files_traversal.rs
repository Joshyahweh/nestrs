//! `stream_file_from_dir` traversal hardening — the safe shape for
//! `/download/:name` handlers where the file name arrives from an
//! untrusted path parameter (percent-decoded before the handler sees it).

#![cfg(feature = "files")]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use std::path::PathBuf;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nestrs-files-traversal-{}-{tag}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn response_body(resp: axum::response::Response) -> Vec<u8> {
    axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("body")
        .to_vec()
}

#[tokio::test]
async fn streams_file_inside_base() {
    let dir = scratch_dir("ok");
    std::fs::write(dir.join("report.txt"), b"file contents").expect("write");
    let resp =
        nestrs::stream_file_from_dir(&dir, "report.txt", "application/octet-stream").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(response_body(resp).await, b"file contents");
}

#[tokio::test]
async fn rejects_traversal_and_absolute_names() {
    let dir = scratch_dir("reject");
    std::fs::write(dir.join("inside.txt"), b"inside").expect("write");
    // Traversal segments, separators, absolute paths, and degenerate names —
    // all 400, never joined onto the base. (The percent-ENCODED form
    // `..%2F..` is covered by the route-level test below: decoding happens
    // in the path extractor, so the helper only ever sees decoded values.)
    for name in [
        "..",
        "../",
        "../../etc/passwd",
        "sub/../../inside.txt",
        "/etc/passwd",
        "..\\..\\windows",
        ".",
        "",
    ] {
        let resp = nestrs::stream_file_from_dir(&dir, name, "application/octet-stream").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "name: {name:?}");
    }
}

#[tokio::test]
async fn missing_file_is_404() {
    let dir = scratch_dir("missing");
    let resp =
        nestrs::stream_file_from_dir(&dir, "no-such-file.txt", "application/octet-stream").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_plant_outside_base_is_404() {
    use std::os::unix::fs::symlink;

    // The attacker can plant files in the upload dir (e.g. via a file
    // upload), but the target outside it is not theirs to serve.
    let dir = scratch_dir("symlink");
    let outside = scratch_dir("outside");
    std::fs::write(outside.join("secret.txt"), b"secret").expect("write");
    symlink(outside.join("secret.txt"), dir.join("innocent.txt")).expect("symlink");

    let resp =
        nestrs::stream_file_from_dir(&dir, "innocent.txt", "application/octet-stream").await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "a symlink pointing outside the base must not be followed"
    );
}

// ---------------------------------------------------------------------------
// End-to-end through a real route: percent-decoding happens in the extractor
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct NameParams {
    name: String,
}

#[derive(Default)]
#[injectable]
struct DownloadState;

#[controller(prefix = "/dl")]
struct DownloadController;

#[routes(state = DownloadState)]
impl DownloadController {
    #[get("/download/:name")]
    async fn download(#[param::param] p: NameParams) -> axum::response::Response {
        let dir = p_download_dir();
        nestrs::stream_file_from_dir(&dir, &p.name, "application/octet-stream").await
    }
}

static DOWNLOAD_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn p_download_dir() -> PathBuf {
    DOWNLOAD_DIR
        .get_or_init(|| {
            let dir = scratch_dir("route");
            std::fs::write(dir.join("safe.txt"), b"safe contents").expect("write");
            dir
        })
        .clone()
}

#[module(controllers = [DownloadController], providers = [DownloadState])]
struct DownloadModule;

#[tokio::test]
async fn route_rejects_percent_encoded_traversal() {
    let app = NestFactory::create::<DownloadModule>().into_router();

    // Safe name round-trips.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/dl/download/safe.txt")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(r.status(), StatusCode::OK);
    let _ = r;

    // `..%2F..%2F` decodes to `../../` inside the path extractor — the
    // helper must reject the decoded value, not the raw segment.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/dl/download/..%2F..%2Fscratch.txt")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);

    // A raw `..` segment is likewise rejected after decoding.
    let r = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/dl/download/..")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}
