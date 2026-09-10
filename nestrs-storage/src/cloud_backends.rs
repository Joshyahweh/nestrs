//! Cloud backends (S3 / GCS / Azure). Each is a thin newtype
//! around the corresponding `object_store` adapter.
//!
//! We only compile this file when at least one of the cloud
//! features is enabled. The unused import lint would otherwise
//! fire on the `object_store::*` re-exports below.

#![cfg(any(feature = "s3", feature = "gcs", feature = "azure"))]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use object_store::path::Path as ObjPath;
use object_store::ObjectStore as _;
use serde::{Deserialize, Serialize};

use crate::presign::PresignedUrl;
use crate::storage::{ListResult, ObjectMeta, Storage, StorageError};

/// `Arc<dyn ObjectStore>` — common inner for all three backends.
type Inner = Arc<dyn object_store::ObjectStore>;

fn map_err(e: object_store::Error) -> StorageError {
    use object_store::Error::*;
    match e {
        NotFound { path, .. } => StorageError::NotFound(path.to_string()),
        PermissionDenied { .. } => StorageError::BadRequest(format!("permission denied: {e}")),
        Unauthenticated { .. } => StorageError::BadRequest(format!("unauthenticated: {e}")),
        _ => StorageError::Transport(e.to_string()),
    }
}

fn path_to_key(p: &ObjPath) -> String {
    p.to_string()
}

fn key_to_path(key: &str) -> Result<ObjPath, StorageError> {
    ObjPath::parse(key).map_err(|e| StorageError::BadRequest(format!("bad key: {e}")))
}

// ---------------------------------------------------------------------------
// S3
// ---------------------------------------------------------------------------

#[cfg(feature = "s3")]
mod s3 {
    use super::*;
    use object_store::aws::AmazonS3Builder;

    #[derive(Clone, Serialize, Deserialize)]
    pub struct S3Config {
        pub bucket: String,
        pub region: String,
        /// Optional endpoint override (LocalStack, MinIO).
        pub endpoint: Option<String>,
        /// Optional access key / secret. When `None`, the SDK
        /// discovers credentials from the environment / IMDS.
        pub access_key: Option<String>,
        pub secret_key: Option<String>,
    }

    impl std::fmt::Debug for S3Config {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // The secret key is never rendered — Debug output flows into
            // logs and error reports. (Serialize stays untouched; it is
            // the config-persistence path, not a logging path.)
            f.debug_struct("S3Config")
                .field("bucket", &self.bucket)
                .field("region", &self.region)
                .field("endpoint", &self.endpoint)
                .field("access_key", &self.access_key)
                .field(
                    "secret_key",
                    &self.secret_key.as_ref().map(|_| "<redacted>"),
                )
                .finish()
        }
    }

    impl S3Config {
        pub fn new(bucket: impl Into<String>, region: impl Into<String>) -> Self {
            Self {
                bucket: bucket.into(),
                region: region.into(),
                endpoint: None,
                access_key: None,
                secret_key: None,
            }
        }

        pub fn with_endpoint(mut self, e: impl Into<String>) -> Self {
            self.endpoint = Some(e.into());
            self
        }

        pub fn with_credentials(
            mut self,
            access_key: impl Into<String>,
            secret_key: impl Into<String>,
        ) -> Self {
            self.access_key = Some(access_key.into());
            self.secret_key = Some(secret_key.into());
            self
        }
    }

    pub struct S3Storage {
        inner: Inner,
        config: S3Config,
    }

    impl std::fmt::Debug for S3Storage {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("S3Storage")
                .field("bucket", &self.config.bucket)
                .field("region", &self.config.region)
                .field("endpoint", &self.config.endpoint)
                .finish()
        }
    }

    impl S3Storage {
        pub fn new(config: S3Config) -> Result<Self, StorageError> {
            let mut b = AmazonS3Builder::new()
                .with_bucket_name(&config.bucket)
                .with_region(&config.region);
            if let Some(ep) = &config.endpoint {
                b = b.with_endpoint(ep);
            }
            if let (Some(ak), Some(sk)) = (&config.access_key, &config.secret_key) {
                b = b.with_access_key_id(ak).with_secret_access_key(sk);
            }
            let inner: Inner = Arc::new(
                b.build()
                    .map_err(|e| StorageError::Misconfigured(format!("S3 builder: {e}")))?,
            );
            Ok(Self { inner, config })
        }

        pub fn config(&self) -> &S3Config {
            &self.config
        }
    }

    #[async_trait]
    impl Storage for S3Storage {
        async fn put(&self, key: &str, data: Bytes) -> Result<(), StorageError> {
            let p = key_to_path(key)?;
            self.inner
                .put(&p, object_store::PutPayload::from(data))
                .await
                .map_err(map_err)?;
            Ok(())
        }
        async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
            let p = key_to_path(key)?;
            let r = self.inner.get(&p).await.map_err(map_err)?;
            r.bytes()
                .await
                .map_err(|e| StorageError::Transport(format!("read body: {e}")))
        }
        async fn delete(&self, key: &str) -> Result<(), StorageError> {
            let p = key_to_path(key)?;
            self.inner.delete(&p).await.map_err(map_err)?;
            Ok(())
        }
        async fn head(&self, key: &str) -> Result<ObjectMeta, StorageError> {
            let p = key_to_path(key)?;
            let m = self.inner.head(&p).await.map_err(map_err)?;
            Ok(ObjectMeta {
                key: path_to_key(&m.location),
                size: m.size as u64,
                last_modified_ms: Some(m.last_modified.timestamp_millis()),
                content_type: None,
                etag: m.e_tag,
            })
        }
        async fn list(&self, prefix: &str) -> Result<ListResult, StorageError> {
            let prefix_path = if prefix.is_empty() {
                ObjPath::from("/")
            } else {
                ObjPath::parse(prefix)
                    .map_err(|e| StorageError::BadRequest(format!("prefix: {e}")))?
            };
            let mut stream = self.inner.list(Some(&prefix_path));
            use futures::StreamExt;
            let mut out = Vec::new();
            while let Some(item) = stream.next().await {
                let r = item.map_err(map_err)?;
                out.push(ObjectMeta {
                    key: path_to_key(&r.location),
                    size: r.size as u64,
                    last_modified_ms: Some(r.last_modified.timestamp_millis()),
                    content_type: None,
                    etag: r.e_tag,
                });
            }
            Ok(ListResult {
                objects: out,
                next_token: None,
            })
        }
        async fn presign_get(
            &self,
            key: &str,
            expires_in: Duration,
        ) -> Result<PresignedUrl, StorageError> {
            // `object_store` 0.12 doesn't expose a public presign
            // helper. The presign implementation lives in
            // `nestrs-storage`'s higher-level helpers (S3 IAMv4
            // signing) — for now we return `PresignUnsupported`
            // and direct users to the SDK-specific path.
            let _ = (key, expires_in);
            Err(StorageError::PresignUnsupported)
        }
        async fn presign_put(
            &self,
            key: &str,
            expires_in: Duration,
        ) -> Result<PresignedUrl, StorageError> {
            let _ = (key, expires_in);
            Err(StorageError::PresignUnsupported)
        }
    }
}

#[cfg(feature = "s3")]
pub use s3::{S3Config, S3Storage};

// ---------------------------------------------------------------------------
// GCS
// ---------------------------------------------------------------------------

#[cfg(feature = "gcs")]
mod gcs {
    use super::*;
    use object_store::gcp::GoogleCloudStorageBuilder;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct GcsConfig {
        pub bucket: String,
        /// Path to the service-account JSON. When `None`, the SDK
        /// uses Application Default Credentials.
        pub service_account_path: Option<String>,
    }

    impl GcsConfig {
        pub fn new(bucket: impl Into<String>) -> Self {
            Self {
                bucket: bucket.into(),
                service_account_path: None,
            }
        }
        pub fn with_service_account(mut self, p: impl Into<String>) -> Self {
            self.service_account_path = Some(p.into());
            self
        }
    }

    pub struct GcsStorage {
        inner: Inner,
        _config: GcsConfig,
    }

    impl std::fmt::Debug for GcsStorage {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("GcsStorage").finish_non_exhaustive()
        }
    }

    impl GcsStorage {
        pub fn new(config: GcsConfig) -> Result<Self, StorageError> {
            let mut b = GoogleCloudStorageBuilder::new().with_bucket_name(&config.bucket);
            if let Some(sa) = &config.service_account_path {
                b = b.with_service_account_path(sa);
            }
            let inner: Inner = Arc::new(
                b.build()
                    .map_err(|e| StorageError::Misconfigured(format!("GCS builder: {e}")))?,
            );
            Ok(Self {
                inner,
                _config: config,
            })
        }
    }

    #[async_trait]
    impl Storage for GcsStorage {
        async fn put(&self, key: &str, data: Bytes) -> Result<(), StorageError> {
            self.inner
                .put(&key_to_path(key)?, object_store::PutPayload::from(data))
                .await
                .map_err(map_err)?;
            Ok(())
        }
        async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
            let r = self.inner.get(&key_to_path(key)?).await.map_err(map_err)?;
            r.bytes()
                .await
                .map_err(|e| StorageError::Transport(format!("read body: {e}")))
        }
        async fn delete(&self, key: &str) -> Result<(), StorageError> {
            self.inner
                .delete(&key_to_path(key)?)
                .await
                .map_err(map_err)?;
            Ok(())
        }
        async fn head(&self, key: &str) -> Result<ObjectMeta, StorageError> {
            let m = self.inner.head(&key_to_path(key)?).await.map_err(map_err)?;
            Ok(ObjectMeta {
                key: path_to_key(&m.location),
                size: m.size as u64,
                last_modified_ms: Some(m.last_modified.timestamp_millis()),
                content_type: None,
                etag: m.e_tag,
            })
        }
        async fn list(&self, prefix: &str) -> Result<ListResult, StorageError> {
            let prefix_path = if prefix.is_empty() {
                ObjPath::from("/")
            } else {
                ObjPath::parse(prefix)
                    .map_err(|e| StorageError::BadRequest(format!("prefix: {e}")))?
            };
            let mut stream = self.inner.list(Some(&prefix_path));
            use futures::StreamExt;
            let mut out = Vec::new();
            while let Some(item) = stream.next().await {
                let r = item.map_err(map_err)?;
                out.push(ObjectMeta {
                    key: path_to_key(&r.location),
                    size: r.size as u64,
                    last_modified_ms: Some(r.last_modified.timestamp_millis()),
                    content_type: None,
                    etag: r.e_tag,
                });
            }
            Ok(ListResult {
                objects: out,
                next_token: None,
            })
        }
        async fn presign_get(
            &self,
            _key: &str,
            _expires_in: Duration,
        ) -> Result<PresignedUrl, StorageError> {
            Err(StorageError::PresignUnsupported)
        }
        async fn presign_put(
            &self,
            _key: &str,
            _expires_in: Duration,
        ) -> Result<PresignedUrl, StorageError> {
            Err(StorageError::PresignUnsupported)
        }
    }
}

#[cfg(feature = "gcs")]
pub use gcs::{GcsConfig, GcsStorage};

// ---------------------------------------------------------------------------
// Azure
// ---------------------------------------------------------------------------

#[cfg(feature = "azure")]
mod azure {
    use super::*;
    use object_store::azure::MicrosoftAzureBuilder;

    #[derive(Clone, Serialize, Deserialize)]
    pub struct AzureConfig {
        pub container: String,
        pub account: String,
        /// Optional access key. When `None`, the SDK uses
        /// `AZURE_STORAGE_ACCOUNT_NAME` + bearer-token auth.
        pub access_key: Option<String>,
    }

    impl std::fmt::Debug for AzureConfig {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // The account access key is never rendered — Debug output flows
            // into logs and error reports. (Serialize stays untouched; it is
            // the config-persistence path, not a logging path.)
            f.debug_struct("AzureConfig")
                .field("container", &self.container)
                .field("account", &self.account)
                .field(
                    "access_key",
                    &self.access_key.as_ref().map(|_| "<redacted>"),
                )
                .finish()
        }
    }

    impl AzureConfig {
        pub fn new(container: impl Into<String>, account: impl Into<String>) -> Self {
            Self {
                container: container.into(),
                account: account.into(),
                access_key: None,
            }
        }
    }

    pub struct AzureBlobStorage {
        inner: Inner,
        _config: AzureConfig,
    }

    impl std::fmt::Debug for AzureBlobStorage {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AzureBlobStorage").finish_non_exhaustive()
        }
    }

    impl AzureBlobStorage {
        pub fn new(config: AzureConfig) -> Result<Self, StorageError> {
            let mut b = MicrosoftAzureBuilder::new()
                .with_container_name(&config.container)
                .with_account(&config.account);
            if let Some(k) = &config.access_key {
                b = b.with_access_key(k);
            }
            let inner: Inner = Arc::new(
                b.build()
                    .map_err(|e| StorageError::Misconfigured(format!("Azure builder: {e}")))?,
            );
            Ok(Self {
                inner,
                _config: config,
            })
        }
    }

    #[async_trait]
    impl Storage for AzureBlobStorage {
        async fn put(&self, key: &str, data: Bytes) -> Result<(), StorageError> {
            self.inner
                .put(&key_to_path(key)?, object_store::PutPayload::from(data))
                .await
                .map_err(map_err)?;
            Ok(())
        }
        async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
            let r = self.inner.get(&key_to_path(key)?).await.map_err(map_err)?;
            r.bytes()
                .await
                .map_err(|e| StorageError::Transport(format!("read body: {e}")))
        }
        async fn delete(&self, key: &str) -> Result<(), StorageError> {
            self.inner
                .delete(&key_to_path(key)?)
                .await
                .map_err(map_err)?;
            Ok(())
        }
        async fn head(&self, key: &str) -> Result<ObjectMeta, StorageError> {
            let m = self.inner.head(&key_to_path(key)?).await.map_err(map_err)?;
            Ok(ObjectMeta {
                key: path_to_key(&m.location),
                size: m.size as u64,
                last_modified_ms: Some(m.last_modified.timestamp_millis()),
                content_type: None,
                etag: m.e_tag,
            })
        }
        async fn list(&self, prefix: &str) -> Result<ListResult, StorageError> {
            let prefix_path = if prefix.is_empty() {
                ObjPath::from("/")
            } else {
                ObjPath::parse(prefix)
                    .map_err(|e| StorageError::BadRequest(format!("prefix: {e}")))?
            };
            let mut stream = self.inner.list(Some(&prefix_path));
            use futures::StreamExt;
            let mut out = Vec::new();
            while let Some(item) = stream.next().await {
                let r = item.map_err(map_err)?;
                out.push(ObjectMeta {
                    key: path_to_key(&r.location),
                    size: r.size as u64,
                    last_modified_ms: Some(r.last_modified.timestamp_millis()),
                    content_type: None,
                    etag: r.e_tag,
                });
            }
            Ok(ListResult {
                objects: out,
                next_token: None,
            })
        }
        async fn presign_get(
            &self,
            _key: &str,
            _expires_in: Duration,
        ) -> Result<PresignedUrl, StorageError> {
            Err(StorageError::PresignUnsupported)
        }
        async fn presign_put(
            &self,
            _key: &str,
            _expires_in: Duration,
        ) -> Result<PresignedUrl, StorageError> {
            Err(StorageError::PresignUnsupported)
        }
    }
}

#[cfg(feature = "azure")]
pub use azure::{AzureBlobStorage, AzureConfig};

#[cfg(test)]
mod debug_redaction_tests {
    // The config structs are feature-gated inside their modules; the tests
    // mirror that so `--all-features` exercises all of them.
    #[cfg(feature = "s3")]
    #[test]
    fn s3_config_debug_never_shows_secret_key() {
        use super::s3::S3Config;
        let config = S3Config {
            bucket: "assets".to_string(),
            region: "us-east-1".to_string(),
            endpoint: None,
            access_key: Some("AKIAEXAMPLE".to_string()),
            secret_key: Some("wJalrXUtnFEMI-DO-NOT-LOG".to_string()),
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("wJalrXUtnFEMI"), "secret key leaked: {rendered}");
        assert!(rendered.contains("<redacted>"), "no redaction marker: {rendered}");
        assert!(rendered.contains("AKIAEXAMPLE"), "access key id should stay visible: {rendered}");
    }

    #[cfg(feature = "azure")]
    #[test]
    fn azure_config_debug_never_shows_access_key() {
        use super::azure::AzureConfig;
        let config = AzureConfig {
            container: "media".to_string(),
            account: "nestrsmedia".to_string(),
            access_key: Some("base64key-DO-NOT-LOG".to_string()),
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("base64key"), "access key leaked: {rendered}");
        assert!(rendered.contains("<redacted>"), "no redaction marker: {rendered}");
        assert!(rendered.contains("nestrsmedia"), "account should stay visible: {rendered}");
    }
}
