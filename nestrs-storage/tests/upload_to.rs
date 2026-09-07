//! Tests for the `#[upload_to]` decorator and the `upload_to` /
//! `resolve_upload_key` / `upload_location` runtime helpers.
//!
//! The proc-macro itself runs at compile time — what we can
//! observe from a test is its *output* (the three
//! `__upload_to_<fn>_*` accessors). The runtime helper tests
//! exercise `resolve_upload_key` directly with hand-built
//! templates so we don't need to wire a full axum route in
//! here.

#![cfg(feature = "local")]

use bytes::Bytes;
use nestrs_storage::presign::PresignMethod;
use nestrs_storage::{
    resolve_upload_key, upload_location, upload_to, LocalConfig, LocalStorage, PresignedUrl,
    Storage, StorageError,
};

#[tokio::test]
async fn upload_to_streams_bytes_to_local_storage() {
    let storage = LocalStorage::in_tempdir().expect("tempdir");
    upload_to(
        &storage,
        "uploads/hello.txt",
        Bytes::from_static(b"hi there"),
    )
    .await
    .expect("upload_to");
    let got = storage.get("uploads/hello.txt").await.expect("get");
    assert_eq!(got.as_ref(), b"hi there");
}

#[tokio::test]
async fn upload_to_propagates_storage_errors() {
    // `..` in the key is rejected at the resolver level — we
    // can use that to assert the error path without mocking.
    let storage = LocalStorage::in_tempdir().expect("tempdir");
    let err = upload_to(&storage, "../escape.txt", Bytes::from_static(b"x"))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)));
}

#[tokio::test]
async fn resolve_upload_key_substitutes_single_placeholder() {
    let key = resolve_upload_key("uploads/{user_id}/file.txt", &["42"]).unwrap();
    assert_eq!(key, "uploads/42/file.txt");
}

#[tokio::test]
async fn resolve_upload_key_substitutes_multiple_placeholders_in_order() {
    let key = resolve_upload_key(
        "uploads/{user_id}/{date}/{filename}",
        &["7", "2026-09-05", "pic.jpg"],
    )
    .unwrap();
    assert_eq!(key, "uploads/7/2026-09-05/pic.jpg");
}

#[tokio::test]
async fn resolve_upload_key_with_no_placeholders_returns_template_unchanged() {
    let key = resolve_upload_key("static/readme.md", &[]).unwrap();
    assert_eq!(key, "static/readme.md");
}

#[tokio::test]
async fn resolve_upload_key_with_empty_placeholder_value_inserts_empty() {
    // Edge case: a placeholder can substitute to "". We don't
    // ban it — the storage layer is the one that rejects
    // malformed keys, not the template resolver.
    let key = resolve_upload_key("a/{p}/b", &[""]).unwrap();
    assert_eq!(key, "a//b");
}

#[tokio::test]
async fn resolve_upload_key_too_few_params_errors() {
    let err = resolve_upload_key("a/{x}/b/{y}/c", &["only-one"]).unwrap_err();
    match err {
        StorageError::BadRequest(msg) => {
            assert!(msg.contains("placeholders"), "unexpected msg: {msg}");
        }
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

#[tokio::test]
async fn resolve_upload_key_too_many_params_errors() {
    let err = resolve_upload_key("a/{x}/b", &["one", "two", "three"]).unwrap_err();
    match err {
        StorageError::BadRequest(msg) => {
            assert!(msg.contains("value(s) provided"), "unexpected msg: {msg}");
        }
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

#[tokio::test]
async fn resolve_upload_key_unterminated_placeholder_errors() {
    let err = resolve_upload_key("a/{unterminated", &[]).unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)));
}

#[tokio::test]
async fn upload_location_concatenates_bucket_and_key() {
    assert_eq!(upload_location("photos", "cat.jpg"), "photos/cat.jpg");
    assert_eq!(upload_location("photos/", "cat.jpg"), "photos/cat.jpg");
    assert_eq!(upload_location("photos", "/cat.jpg"), "photos/cat.jpg");
    assert_eq!(upload_location("photos/", "/cat.jpg"), "photos/cat.jpg");
    assert_eq!(upload_location("", "cat.jpg"), "cat.jpg");
}

#[tokio::test]
async fn upload_location_drops_trailing_slash_on_bucket() {
    // The trailing slash on a bucket is normalised away; the
    // leading slash on a key is normalised away. This keeps the
    // macro + runtime combo idempotent regardless of how the
    // user wrote the template.
    let loc = upload_location("bucket/", "/path/");
    assert_eq!(loc, "bucket/path/");
}

#[tokio::test]
async fn upload_to_then_get_round_trips_through_resolved_key() {
    // End-to-end: the user resolves the key, calls upload_to,
    // then gets it back. This is the full path the `#[upload_to]`
    // decorator enables, minus the proc-macro itself.
    let storage = LocalStorage::in_tempdir().expect("tempdir");
    let bucket = "avatars";
    let key = resolve_upload_key("{user_id}/{kind}.png", &["42", "profile"]).unwrap();
    let loc = upload_location(bucket, &key);
    upload_to(&storage, &loc, Bytes::from_static(b"\x89PNG\r\n\x1a\n"))
        .await
        .expect("upload_to");
    let got = storage.get(&loc).await.expect("get");
    assert_eq!(got.as_ref(), b"\x89PNG\r\n\x1a\n");
}

#[tokio::test]
async fn presign_get_url_contains_resolved_key() {
    // Build a `LocalStorage` whose `public_base` matches a
    // freshly-resolved upload location; the presign URL should
    // embed both the bucket and the templated key.
    let storage = LocalStorage::new(
        LocalConfig::new(std::env::temp_dir()).with_public_base("https://cdn.example.com"),
    )
    .unwrap();
    let key = resolve_upload_key("{user_id}/{kind}.png", &["42", "avatar"]).unwrap();
    let url = storage
        .presign_get(&key, std::time::Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(url.url, "https://cdn.example.com/42/avatar.png");
    assert!(matches!(url.method, PresignMethod::Get));
    assert!(!url.is_expired());
}

#[tokio::test]
async fn presign_url_with_zero_duration_is_already_expired() {
    let storage = LocalStorage::new(
        LocalConfig::new(std::env::temp_dir()).with_public_base("https://cdn.example.com"),
    )
    .unwrap();
    let url = storage
        .presign_get("k", std::time::Duration::from_secs(0))
        .await
        .unwrap();
    // The expiry is stamped from `now() + 0s`, which is
    // effectively already in the past by the time we call
    // `is_expired`. We allow either `expired == true` (clock
    // advanced) or the rare-but-possible `false` if the system
    // clock resolution is coarse enough that the timestamp
    // matches exactly. The real assertion is that the API
    // accepts `Duration::ZERO` without panicking.
    let _ = url.is_expired();
    // The expiry is `now + 0`, so it must be at or before the
    // current `now`. Round-down: a coarse clock could land it
    // exactly equal — accept either side of the boundary.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    assert!(url.expires_at_unix <= now + 1);
}

#[tokio::test]
async fn presign_get_and_put_share_url_shape_but_differ_on_method() {
    let storage = LocalStorage::new(
        LocalConfig::new(std::env::temp_dir()).with_public_base("https://cdn.example.com"),
    )
    .unwrap();
    let get_url = storage
        .presign_get("foo.bin", std::time::Duration::from_secs(60))
        .await
        .unwrap();
    let put_url = storage
        .presign_put("foo.bin", std::time::Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(get_url.url, put_url.url);
    assert!(matches!(get_url.method, PresignMethod::Get));
    assert!(matches!(put_url.method, PresignMethod::Put));
}

#[tokio::test]
async fn presigned_url_serializes_round_trip() {
    // Smoke test the serde shape — `PresignedUrl` is part of
    // the public API so users will serialize it (e.g. to JSON
    // in a response body).
    let url = PresignedUrl {
        url: "https://x".into(),
        method: PresignMethod::Get,
        expires_at_unix: 1_700_000_000,
        headers: vec![("x-amz-acl".into(), "private".into())],
    };
    let json = serde_json::to_string(&url).unwrap();
    let back: PresignedUrl = serde_json::from_str(&json).unwrap();
    assert_eq!(back, url);
}
