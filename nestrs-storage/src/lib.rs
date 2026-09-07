//! `nestrs-storage` — multi-cloud object storage for nestrs.
//!
//! The crate exposes a single `Storage` trait that abstracts the four
//! common backends (Local filesystem, S3, GCS, Azure Blob) over a
//! uniform async API. Each backend is a thin newtype around the
//! corresponding `object_store` adapter (where applicable) or a
//! direct filesystem implementation (for `LocalStorage`).
//!
//! Backends are gated behind feature flags so the dep tree stays
//! minimal: the `local` feature adds nothing (we hand-roll the
//! filesystem adapter); `s3` / `gcs` / `azure` each pull in the
//! relevant `object_store` feature and its native-tls / rustls
//! variant. `all` is a convenience for the "give me everything"
//! case.
//!
//! Public surface:
//! ```text
//!   storage::{Storage, StorageError, ObjectMeta, ListResult}
//!   backends::{LocalStorage, S3Storage, GcsStorage, AzureBlobStorage}
//!   upload::upload_to
//!   presign::{PresignedUrl, PresignMethod, presign_get, presign_put}
//! ```

pub mod presign;
pub mod storage;
pub mod upload;

#[cfg(feature = "local")]
pub mod backends;
#[cfg(any(feature = "s3", feature = "gcs", feature = "azure"))]
pub mod cloud_backends;

#[cfg(feature = "local")]
pub use backends::{LocalConfig, LocalStorage};
#[cfg(feature = "azure")]
pub use cloud_backends::{AzureBlobStorage, AzureConfig};
#[cfg(feature = "gcs")]
pub use cloud_backends::{GcsConfig, GcsStorage};
#[cfg(feature = "s3")]
pub use cloud_backends::{S3Config, S3Storage};
pub use presign::{presign_get, presign_put, PresignMethod, PresignedUrl};
pub use storage::{ListResult, ObjectMeta, Storage, StorageError};
pub use upload::{resolve_upload_key, upload_location, upload_to};
