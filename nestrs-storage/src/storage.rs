//! The `Storage` trait and its error / metadata types.
//!
//! The trait is intentionally narrow: `put` / `get` / `delete` /
//! `head` / `list` plus `presign_get` / `presign_put`. Cloud-specific
//! niceties (multipart, range reads, server-side copy) are layered
//! on via backend-specific methods where needed.

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::presign::{PresignMethod, PresignedUrl};

/// Errors a `Storage` backend can return. Backend-specific
/// failures (S3 throttling, Azure auth) wrap into one of the
/// variants below.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Network / IO / transient transport error. Retryable.
    #[error("transport error: {0}")]
    Transport(String),
    /// The named object doesn't exist. Maps to HTTP 404.
    #[error("object `{0}` not found")]
    NotFound(String),
    /// The bucket / container / prefix doesn't exist or isn't
    /// accessible.
    #[error("bucket `{0}` not accessible: {1}")]
    Bucket(String, String),
    /// Backend rejected the request (bad credentials, malformed
    /// key, etc.). NOT retryable.
    #[error("bad request: {0}")]
    BadRequest(String),
    /// Backend misconfiguration (missing client, unset region).
    /// Programming error, not retryable.
    #[error("misconfigured: {0}")]
    Misconfigured(String),
    /// The `presign_*` call was issued against a backend that
    /// doesn't support pre-signed URLs.
    #[error("backend does not support pre-signed URLs")]
    PresignUnsupported,
}

/// Metadata for a stored object. Returned by `head` and as part of
/// `list` results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMeta {
    /// The object's key (path within the bucket / container).
    pub key: String,
    /// Object size in bytes.
    pub size: u64,
    /// Last-modified timestamp (milliseconds since the Unix epoch).
    /// `None` if the backend doesn't track it.
    pub last_modified_ms: Option<i64>,
    /// Content-Type, if the backend recorded one.
    pub content_type: Option<String>,
    /// ETag / MD5 / generation — opaque to callers.
    pub etag: Option<String>,
}

/// One page of a `list` call. Backends may paginate; the default
/// `Storage::list` impls return everything in one go for the
/// `LocalStorage` and a single page for the cloud backends.
#[derive(Debug, Clone, Default)]
pub struct ListResult {
    pub objects: Vec<ObjectMeta>,
    /// Continuation token; if `Some`, the caller should re-issue
    /// the `list` with this token to fetch the next page.
    pub next_token: Option<String>,
}

/// The `Storage` trait. Backends implement it; the framework uses
/// the trait object via `Arc<dyn Storage>` when wiring routes.
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    /// Write `data` to `key`, overwriting if it exists.
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StorageError>;

    /// Read the object at `key`. Returns `StorageError::NotFound`
    /// on miss.
    async fn get(&self, key: &str) -> Result<Bytes, StorageError>;

    /// Delete the object at `key`. Missing keys are not an error.
    async fn delete(&self, key: &str) -> Result<(), StorageError>;

    /// Fetch the metadata for `key` without reading the body.
    async fn head(&self, key: &str) -> Result<ObjectMeta, StorageError>;

    /// List objects with the given prefix. Default impls may return
    /// the full list in one go.
    async fn list(&self, prefix: &str) -> Result<ListResult, StorageError>;

    /// Pre-sign a GET URL. The returned URL is valid for
    /// `expires_in`; the caller can hand it to a client to download
    /// the object without further authentication.
    async fn presign_get(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<PresignedUrl, StorageError>;

    /// Pre-sign a PUT URL. The caller can `PUT` the body to the
    /// returned URL without further authentication.
    async fn presign_put(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<PresignedUrl, StorageError>;

    /// Indicate which `PresignMethod` this backend supports. Used
    /// by the `presign_*` helpers to choose the right call. Default:
    /// both.
    fn presign_methods(&self) -> PresignMethod {
        PresignMethod::GetPut
    }
}
