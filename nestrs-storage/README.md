# nestrs-storage

**Multi-cloud object storage** for [nestrs](https://crates.io/crates/nestrs): one async `Storage` trait over **Local filesystem**, **S3**, **GCS**, and **Azure Blob** backends, with presigned-URL helpers and the `#[upload_to]` decorator. Built on [`object_store`](https://crates.io/crates/object_store).

`nestrs-storage` is a separate crate, not a feature flag on `nestrs` — add it as its own dependency.

**Docs:** [docs.rs/nestrs-storage](https://docs.rs/nestrs-storage) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-storage = { version = "1.5.0", features = ["local"] }
# or: features = ["s3"], ["gcs"], ["azure"], or ["all"]
```

The `local` backend hand-rolls the filesystem adapter (no extra deps); `s3` / `gcs` / `azure` each pull in the relevant `object_store` feature. `all` is the give-me-everything case.

## Example: local backend

```rust
use bytes::Bytes;
use nestrs_storage::{LocalConfig, LocalStorage, Storage};

#[tokio::main]
async fn main() -> Result<(), nestrs_storage::StorageError> {
    let storage = LocalStorage::new(LocalConfig::new("data"))?;

    storage
        .put("uploads/hello.txt", Bytes::from_static(b"hi there"))
        .await?;
    let got = storage.get("uploads/hello.txt").await?;
    assert_eq!(got.as_ref(), b"hi there");

    for meta in storage.list("uploads/").await?.objects {
        println!("{} ({} bytes)", meta.key, meta.size);
    }
    Ok(())
}
```

## Example: S3 (MinIO / LocalStack / AWS)

```rust
use nestrs_storage::{S3Config, S3Storage};

let storage = S3Storage::new(
    S3Config::new("my-bucket", "us-east-1")
        .with_endpoint("http://localhost:9000")       // MinIO / LocalStack
        .with_credentials(access_key, secret_key),
)?;
```

GCS (`GcsConfig::new(bucket)`, `.with_service_account(path)`) and Azure (`AzureConfig::new(container, account)`) follow the same shape.

## The `Storage` trait

| Method | Purpose |
|---|---|
| `put(key, data)` | Write bytes, overwriting if present |
| `get(key)` | Read bytes (`StorageError::NotFound` on miss) |
| `delete(key)` | Delete; missing keys are not an error |
| `head(key)` | Metadata (`ObjectMeta`) without the body |
| `list(prefix)` | List objects under a prefix |
| `presign_get(key, expires_in)` / `presign_put(...)` | Pre-signed URLs (`PresignedUrl { url, method, expires_at_unix, headers }`) |

Backends advertise what they can pre-sign via `presign_methods()`; the free helpers `presign_get` / `presign_put` pick the right call and return `PresignUnsupported` where the backend can't sign.

> **Note:** Local pre-signing is a development affordance — it renders `{public_base}/{key}` with an expiry query param (set via `LocalConfig::with_public_base`) rather than a real signature. Production presigning uses S3 / GCS / Azure.

## Uploads

`#[upload_to("bucket", "prefix/{user_id}/file-{file_id}")]` marks a route handler as an upload endpoint; the template placeholders are resolved at runtime by `resolve_upload_key`, and `upload_to(storage, &key, body)` streams the bytes to any backend:

```rust
let key = resolve_upload_key("uploads/{user_id}/avatar.png", &["42"])?;
upload_to(&storage, &key, body).await?;
```

`upload_location(bucket, key)` renders the display path for responses.

## Features

| Feature | Purpose |
|---|---|
| `local` | Local filesystem backend (`LocalStorage`) — no external services |
| `s3` | S3 / S3-compatible (AWS, MinIO, LocalStack) |
| `gcs` | Google Cloud Storage |
| `azure` | Azure Blob Storage |
| `all` | All four backends |

## License

MIT OR Apache-2.0.
