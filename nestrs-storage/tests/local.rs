//! `LocalStorage` round-trip tests. No external service required.

#![cfg(feature = "local")]

use std::time::Duration;

use bytes::Bytes;
use nestrs_storage::presign::PresignMethod;
use nestrs_storage::{LocalConfig, LocalStorage, Storage, StorageError};

fn fresh() -> LocalStorage {
    LocalStorage::in_tempdir().expect("TempDir create")
}

#[tokio::test]
async fn local_round_trip() {
    let s = fresh();
    s.put("a/b/c.txt", Bytes::from_static(b"hello world"))
        .await
        .expect("put");
    let got = s.get("a/b/c.txt").await.expect("get");
    assert_eq!(got.as_ref(), b"hello world");
}

#[tokio::test]
async fn local_get_missing_key_returns_not_found() {
    let s = fresh();
    let err = s.get("nope.txt").await.unwrap_err();
    assert!(matches!(err, StorageError::NotFound(_)));
}

#[tokio::test]
async fn local_delete_is_idempotent() {
    let s = fresh();
    s.put("x.txt", Bytes::from_static(b"y")).await.unwrap();
    s.delete("x.txt").await.unwrap();
    // Second delete is a no-op (not an error).
    s.delete("x.txt").await.unwrap();
    // Re-get still 404.
    assert!(matches!(
        s.get("x.txt").await.unwrap_err(),
        StorageError::NotFound(_)
    ));
}

#[tokio::test]
async fn local_head_reports_size_and_mtime() {
    let s = fresh();
    s.put("h.txt", Bytes::from_static(b"abcdef")).await.unwrap();
    let m = s.head("h.txt").await.unwrap();
    assert_eq!(m.size, 6);
    assert!(m.last_modified_ms.is_some());
}

#[tokio::test]
async fn local_head_missing_returns_not_found() {
    let s = fresh();
    assert!(matches!(
        s.head("nope.txt").await.unwrap_err(),
        StorageError::NotFound(_)
    ));
}

#[tokio::test]
async fn local_list_prefix_returns_objects() {
    let s = fresh();
    s.put("photos/a.jpg", Bytes::from_static(b"a"))
        .await
        .unwrap();
    s.put("photos/b.jpg", Bytes::from_static(b"b"))
        .await
        .unwrap();
    s.put("docs/readme.md", Bytes::from_static(b"d"))
        .await
        .unwrap();
    let listing = s.list("photos/").await.unwrap();
    let keys: Vec<String> = listing.objects.iter().map(|o| o.key.clone()).collect();
    assert!(keys.iter().any(|k| k.ends_with("a.jpg")));
    assert!(keys.iter().any(|k| k.ends_with("b.jpg")));
    assert!(!keys.iter().any(|k| k.ends_with("readme.md")));
}

#[tokio::test]
async fn local_presign_get_returns_url_that_resolves() {
    // For local, presign just builds a URL with a public_base.
    // We can't really GET it without a server, but we can assert
    // the URL is well-formed and has an expiry in the future.
    let s = LocalStorage::new(
        LocalConfig::new(std::env::temp_dir()).with_public_base("https://local.test/storage"),
    )
    .unwrap();
    let url = s
        .presign_get("photos/cat.jpg", Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(url.url, "https://local.test/storage/photos/cat.jpg");
    assert!(matches!(url.method, PresignMethod::Get));
    assert!(!url.is_expired());
}

#[tokio::test]
async fn local_presign_put_supports_write_urls() {
    let s = LocalStorage::new(
        LocalConfig::new(std::env::temp_dir()).with_public_base("https://local.test"),
    )
    .unwrap();
    let url = s
        .presign_put("upload.bin", Duration::from_secs(120))
        .await
        .unwrap();
    assert_eq!(url.url, "https://local.test/upload.bin");
    assert!(matches!(url.method, PresignMethod::Put));
}

#[tokio::test]
async fn local_presign_without_public_base_errors() {
    let s = fresh();
    let err = s
        .presign_get("k", Duration::from_secs(60))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::Misconfigured(_)));
}

#[tokio::test]
async fn local_rejects_dotdot_in_key() {
    let s = fresh();
    // Normalize ".." out via the resolver, but it rejects.
    let err = s
        .put("../etc/passwd", Bytes::from_static(b"p"))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)));
}

#[tokio::test]
async fn local_overwrite_replaces_contents() {
    let s = fresh();
    s.put("f.txt", Bytes::from_static(b"v1")).await.unwrap();
    s.put("f.txt", Bytes::from_static(b"v2")).await.unwrap();
    let got = s.get("f.txt").await.unwrap();
    assert_eq!(got.as_ref(), b"v2");
    // Size is updated.
    assert_eq!(s.head("f.txt").await.unwrap().size, 2);
}
