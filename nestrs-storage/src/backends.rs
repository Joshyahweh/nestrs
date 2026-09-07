//! `LocalStorage` — the on-disk backend. No external service
//! required; backed by a `tempfile::TempDir` or any user-supplied
//! directory. Useful for tests, single-host deployments, and
//! "pre-signed URL" smoke tests where you don't want to spin up a
//! cloud emulator.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::presign::{PresignMethod, PresignedUrl};
use crate::storage::{ListResult, ObjectMeta, Storage, StorageError};

/// Configuration for the local backend. `root` is the directory
/// under which all keys are stored; the constructor creates it if
/// it doesn't exist.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalConfig {
    pub root: PathBuf,
    /// Base URL for pre-signed URLs. The local backend doesn't
    /// actually sign anything — it generates `format!("{base}/{key}")`
    /// and stamps the URL with a `?expires=…` query param. The
    /// framework's local presign resolver can then check the
    /// expiry and serve the file. This is a development affordance
    /// only; production uses S3 / GCS / Azure.
    #[serde(default)]
    pub public_base: Option<String>,
}

impl LocalConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            public_base: None,
        }
    }

    pub fn with_public_base(mut self, base: impl Into<String>) -> Self {
        self.public_base = Some(base.into());
        self
    }
}

/// The local backend. `Arc<Mutex<…>>` is held just to keep the
/// `TempDir` alive (when we own one) so the test directory isn't
/// deleted out from under us.
pub struct LocalStorage {
    config: LocalConfig,
    /// Holds a `TempDir` when this storage was constructed without
    /// an explicit root.
    _tempdir: Option<Arc<Mutex<tempfile::TempDir>>>,
}

impl std::fmt::Debug for LocalStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalStorage")
            .field("root", &self.config.root)
            .field("public_base", &self.config.public_base)
            .finish()
    }
}

impl LocalStorage {
    /// Build a local backend rooted at `root`. The directory is
    /// created if it doesn't exist.
    pub fn new(config: LocalConfig) -> Result<Self, StorageError> {
        std::fs::create_dir_all(&config.root).map_err(|e| {
            StorageError::Misconfigured(format!("create_dir_all({:?}) failed: {e}", config.root))
        })?;
        Ok(Self {
            config,
            _tempdir: None,
        })
    }

    /// Build a local backend in a fresh `tempfile::TempDir`. The
    /// directory is automatically cleaned up on drop. Useful for
    /// tests.
    pub fn in_tempdir() -> Result<Self, StorageError> {
        let tmp = tempfile::TempDir::new()
            .map_err(|e| StorageError::Misconfigured(format!("TempDir::new failed: {e}")))?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(&root)
            .map_err(|e| StorageError::Misconfigured(format!("create_dir_all failed: {e}")))?;
        Ok(Self {
            config: LocalConfig::new(root),
            _tempdir: Some(Arc::new(Mutex::new(tmp))),
        })
    }

    pub fn config(&self) -> &LocalConfig {
        &self.config
    }

    /// Resolve a key to an absolute path on disk. The key is
    /// appended to `root` after a single normalization step (no
    /// `..` traversal allowed — `..` is rejected as a `BadRequest`).
    fn resolve(&self, key: &str) -> Result<PathBuf, StorageError> {
        let normalized = normalize_key(key);
        let path = self.config.root.join(&normalized);
        // Reject `..` segments that would escape the root.
        for component in path.components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err(StorageError::BadRequest(format!(
                    "key `{key}` contains `..`"
                )));
            }
        }
        Ok(path)
    }
}

/// Strip a leading `/` and replace any remaining `/` with the
/// platform separator. Keys are slash-delimited, like S3.
fn normalize_key(key: &str) -> PathBuf {
    let trimmed = key.trim_start_matches('/');
    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    let mut path = PathBuf::new();
    for p in parts {
        path.push(p);
    }
    path
}

#[async_trait]
impl Storage for LocalStorage {
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StorageError> {
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::Transport(format!("mkdir {:?}: {e}", parent)))?;
        }
        let mut f = fs::File::create(&path)
            .await
            .map_err(|e| StorageError::Transport(format!("create {:?}: {e}", path)))?;
        f.write_all(&data)
            .await
            .map_err(|e| StorageError::Transport(format!("write {:?}: {e}", path)))?;
        f.flush()
            .await
            .map_err(|e| StorageError::Transport(format!("flush {:?}: {e}", path)))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
        let path = self.resolve(key)?;
        match fs::read(&path).await {
            Ok(b) => Ok(Bytes::from(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StorageError::NotFound(key.to_string()))
            }
            Err(e) => Err(StorageError::Transport(format!("read {:?}: {e}", path))),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.resolve(key)?;
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Transport(format!("rm {:?}: {e}", path))),
        }
    }

    async fn head(&self, key: &str) -> Result<ObjectMeta, StorageError> {
        let path = self.resolve(key)?;
        let meta = fs::metadata(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Transport(format!("stat {:?}: {e}", path))
            }
        })?;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        Ok(ObjectMeta {
            key: key.to_string(),
            size: meta.len(),
            last_modified_ms: mtime_ms,
            content_type: None,
            etag: None,
        })
    }

    async fn list(&self, prefix: &str) -> Result<ListResult, StorageError> {
        let prefix_path = self.config.root.join(normalize_key(prefix));
        let mut out = Vec::new();
        let mut stack = vec![prefix_path];
        while let Some(dir) = stack.pop() {
            let mut rd = fs::read_dir(&dir).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::NotFound(prefix.to_string())
                } else {
                    StorageError::Transport(format!("read_dir {:?}: {e}", dir))
                }
            })?;
            while let Some(entry) = rd
                .next_entry()
                .await
                .map_err(|e| StorageError::Transport(format!("next_entry: {e}")))?
            {
                let ft = entry
                    .file_type()
                    .await
                    .map_err(|e| StorageError::Transport(format!("file_type: {e}")))?;
                if ft.is_dir() {
                    stack.push(entry.path());
                } else if ft.is_file() {
                    let rel = entry
                        .path()
                        .strip_prefix(&self.config.root)
                        .map_err(|e| StorageError::Misconfigured(format!("strip_prefix: {e}")))?
                        .to_string_lossy()
                        .replace('\\', "/");
                    let m = entry
                        .metadata()
                        .await
                        .map_err(|e| StorageError::Transport(format!("metadata: {e}")))?;
                    let mtime_ms = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64);
                    out.push(ObjectMeta {
                        key: rel,
                        size: m.len(),
                        last_modified_ms: mtime_ms,
                        content_type: None,
                        etag: None,
                    });
                }
            }
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
        let base = self
            .config
            .public_base
            .as_deref()
            .ok_or(StorageError::Misconfigured(
                "LocalStorage presign requires public_base".into(),
            ))?;
        let url = format!("{}/{}", base.trim_end_matches('/'), key);
        Ok(PresignedUrl {
            url,
            method: PresignMethod::Get,
            expires_at_unix: PresignedUrl::expires_at_from_now(expires_in),
            headers: Vec::new(),
        })
    }

    async fn presign_put(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<PresignedUrl, StorageError> {
        let base = self
            .config
            .public_base
            .as_deref()
            .ok_or(StorageError::Misconfigured(
                "LocalStorage presign requires public_base".into(),
            ))?;
        let url = format!("{}/{}", base.trim_end_matches('/'), key);
        Ok(PresignedUrl {
            url,
            method: PresignMethod::Put,
            expires_at_unix: PresignedUrl::expires_at_from_now(expires_in),
            headers: Vec::new(),
        })
    }
}

// Allow `LocalStorage::config` to be referenced from tests that
// resolve the root path. Re-export the path utility for tests.
pub fn local_root(storage: &LocalStorage) -> &Path {
    &storage.config.root
}
