//! Pre-signed URL types and helpers.
//!
//! A `PresignedUrl` is what the cloud SDKs return: an HTTPS URL
//! with an embedded signature (and expiry timestamp) that the
//! client uses to upload / download an object without further
//! authentication. We model both GET and PUT (the two methods most
//! providers support); multipart upload / range GET have their own
//! types in their respective backends.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::storage::StorageError;

/// A pre-signed URL. `expires_at_unix` is the wall-clock timestamp
/// (seconds since the Unix epoch) at which the signature becomes
/// invalid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresignedUrl {
    pub url: String,
    pub method: PresignMethod,
    pub expires_at_unix: u64,
    /// Required request headers (e.g. `content-type` for a PUT).
    pub headers: Vec<(String, String)>,
}

/// Which HTTP method(s) a backend can pre-sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresignMethod {
    /// Only GET (read-only backends).
    Get,
    /// Only PUT (write-only backends).
    Put,
    /// Both GET and PUT.
    GetPut,
    /// Neither — `presign_*` always returns `PresignUnsupported`.
    None,
}

impl PresignedUrl {
    /// Compute the expiry timestamp for a `Duration` from now.
    pub fn expires_at_from_now(expires_in: Duration) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now + expires_in.as_secs()
    }

    /// `true` if the URL has expired.
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now >= self.expires_at_unix
    }
}

/// Pre-sign a GET. Convenience wrapper.
pub async fn presign_get<S: crate::storage::Storage + ?Sized>(
    storage: &S,
    key: &str,
    expires_in: Duration,
) -> Result<PresignedUrl, StorageError> {
    storage.presign_get(key, expires_in).await
}

/// Pre-sign a PUT. Convenience wrapper.
pub async fn presign_put<S: crate::storage::Storage + ?Sized>(
    storage: &S,
    key: &str,
    expires_in: Duration,
) -> Result<PresignedUrl, StorageError> {
    storage.presign_put(key, expires_in).await
}
