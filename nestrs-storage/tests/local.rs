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
    let err = s.get("foo/../../etc/passwd").await.unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)));
}

#[tokio::test]
async fn local_list_rejects_parent_traversal() {
    let s = fresh();
    s.put("inside.txt", Bytes::from_static(b"ok"))
        .await
        .unwrap();
    // `list` used to join `..` without the ParentDir check `put`/`get` had,
    // so a caller-supplied prefix could read the parent directory.
    let err = s.list("..").await.unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)));
    let err = s.list("photos/../../outside").await.unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)));
    let listing = s.list("").await.unwrap();
    assert!(listing
        .objects
        .iter()
        .any(|o| o.key.ends_with("inside.txt")));
}

#[tokio::test]
async fn local_presign_rejects_traversal_and_encodes_metacharacters() {
    let s = LocalStorage::new(
        LocalConfig::new(std::env::temp_dir()).with_public_base("https://local.test/storage"),
    )
    .unwrap();
    let err = s
        .presign_get("../secret", Duration::from_secs(60))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)));
    let url = s
        .presign_put("photos/a b?.jpg", Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(url.url, "https://local.test/storage/photos/a%20b%3F.jpg");
    assert!(!url.url.contains(' ') && !url.url.contains('?'));
}

#[cfg(unix)]
#[tokio::test]
async fn local_rejects_symlink_escape() {
    let s = fresh();
    let root = s.config().root.clone();
    let outside = tempfile::tempdir().expect("outside");
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, b"topsecret").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
    let planted = outside.path().join("planted.txt");
    std::os::unix::fs::symlink(&planted, root.join("dangling")).unwrap();

    let err = s.get("link/secret.txt").await.unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)), "{err}");
    let err = s.list("link").await.unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)), "{err}");
    let err = s
        .put("link/planted.txt", Bytes::from_static(b"x"))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)), "{err}");
    let err = s
        .put("dangling", Bytes::from_static(b"x"))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)), "{err}");
    assert!(!planted.exists(), "put must not create the symlink target");
}

#[tokio::test]
async fn local_list_rejects_parent_prefix() {
    let s = fresh();
    s.put("photos/a.jpg", Bytes::from_static(b"a"))
        .await
        .unwrap();
    for prefix in ["..", "../", "photos/../../etc", "..\\..\\secret"] {
        let err = s.list(prefix).await.unwrap_err();
        assert!(
            matches!(err, StorageError::BadRequest(_)),
            "prefix {prefix:?} leaked: {err}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn local_rejects_symlink_that_leaves_the_root() {
    let s = fresh();
    let root = s.config().root.canonicalize().unwrap();
    let outside = root.parent().unwrap().join(format!(
        "nestrs-outside-{}-{}",
        std::process::id(),
        root.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();

    let get_err = s.get("linked/secret.txt").await.unwrap_err();
    assert!(
        matches!(get_err, StorageError::BadRequest(_)),
        "get followed a symlink: {get_err}"
    );
    let list_err = s.list("linked").await.unwrap_err();
    assert!(
        matches!(list_err, StorageError::BadRequest(_)),
        "list followed a symlink: {list_err}"
    );
    let put_err = s
        .put("linked/pwned.txt", Bytes::from_static(b"x"))
        .await
        .unwrap_err();
    assert!(matches!(put_err, StorageError::BadRequest(_)));
    assert!(
        !outside.join("pwned.txt").exists(),
        "put wrote through the symlink"
    );

    let _ = std::fs::remove_dir_all(&outside);
}

#[tokio::test]
async fn local_presign_rejects_traversal_and_encodes_reserved_characters() {
    let s = LocalStorage::new(
        LocalConfig::new(std::env::temp_dir()).with_public_base("https://local.test/storage"),
    )
    .unwrap();
    let err = s
        .presign_get("../etc/passwd", Duration::from_secs(60))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::BadRequest(_)));
    let url = s
        .presign_get("a/b?.jpg", Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(url.url, "https://local.test/storage/a/b%3F.jpg");
    assert!(!url.url.contains('?'));
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
