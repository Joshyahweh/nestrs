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
    /// Base URL for pre-signed URLs. The local backend does not sign
    /// the URL or embed an expiry in it — `expires_at_unix` on the
    /// returned [`PresignedUrl`] is the only
    /// expiry, and callers must enforce it. The key is percent-encoded
    /// onto this base. Development affordance only; production uses
    /// S3 / GCS / Azure.
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

    /// Resolve a key to an absolute path on disk that cannot leave `root`.
    ///
    /// `..`, `.`, NUL, and `\` are rejected. Existing path components are
    /// canonicalized, so a symlink planted inside the root that points
    /// outside it is `BadRequest` rather than followed.
    fn resolve(&self, key: &str) -> Result<PathBuf, StorageError> {
        let normalized = normalize_key(key)?;
        if normalized.as_os_str().is_empty() {
            return Err(StorageError::BadRequest(
                "storage key must not be empty".into(),
            ));
        }
        let root = self.canonical_root()?;
        confine_under(&root, &normalized)
    }

    fn canonical_root(&self) -> Result<PathBuf, StorageError> {
        self.config.root.canonicalize().map_err(|e| {
            StorageError::Misconfigured(format!(
                "canonicalize storage root {:?}: {e}",
                self.config.root
            ))
        })
    }

    /// Directory `list` should walk. An empty prefix is the storage root.
    fn resolve_prefix(&self, prefix: &str) -> Result<PathBuf, StorageError> {
        let root = self.canonical_root()?;
        let normalized = normalize_key(prefix)?;
        if normalized.as_os_str().is_empty() {
            Ok(root)
        } else {
            confine_under(&root, &normalized)
        }
    }
}

/// Slash-delimited object key → relative path. Rejects traversal and
/// separator tricks (`..`, `\`, NUL; `:` on Windows, where it names an
/// alternate data stream or a drive prefix).
fn normalize_key(key: &str) -> Result<PathBuf, StorageError> {
    if key.contains('\0') {
        return Err(StorageError::BadRequest("storage key contains NUL".into()));
    }
    if key.contains('\\') {
        return Err(StorageError::BadRequest("storage key contains `\\`".into()));
    }
    if cfg!(windows) && key.contains(':') {
        return Err(StorageError::BadRequest("storage key contains `:`".into()));
    }
    let trimmed = key.trim_start_matches('/');
    let mut path = PathBuf::new();
    for part in trimmed.split('/').filter(|part| !part.is_empty()) {
        if part == ".." || part == "." {
            return Err(StorageError::BadRequest(format!(
                "storage key `{key}` contains `{part}`"
            )));
        }
        path.push(part);
    }
    Ok(path)
}

/// Join `normalized` onto the canonical `root`. Follow a component only
/// when its canonical path is still inside `root`.
fn confine_under(root: &Path, normalized: &Path) -> Result<PathBuf, StorageError> {
    let mut resolved = root.to_path_buf();
    for component in normalized.components() {
        let name = match component {
            std::path::Component::Normal(name) => name,
            _ => {
                return Err(StorageError::BadRequest(
                    "storage key contains a disallowed path component".into(),
                ));
            }
        };
        let next = resolved.join(name);
        // `symlink_metadata` sees the link itself. A broken link still
        // exists; `exists()` would report it missing and we would later
        // open the lexical path, which follows the link.
        if next.symlink_metadata().is_ok() {
            let canon = next.canonicalize().map_err(|_| {
                StorageError::BadRequest("storage key escapes the storage root".into())
            })?;
            if !canon.starts_with(root) {
                return Err(StorageError::BadRequest(
                    "storage key escapes the storage root".into(),
                ));
            }
            resolved = canon;
        } else {
            resolved = next;
        }
    }
    if !resolved.starts_with(root) {
        return Err(StorageError::BadRequest(
            "storage key escapes the storage root".into(),
        ));
    }
    Ok(resolved)
}

/// Build a local presigned URL. The key is the same restricted object key
/// as `put`/`get`, and each segment is percent-encoded so `?` / `#` cannot
/// rewrite the URL.
fn presigned_local_url(base: &str, key: &str) -> Result<String, StorageError> {
    let normalized = normalize_key(key)?;
    if normalized.as_os_str().is_empty() {
        return Err(StorageError::BadRequest(
            "storage key must not be empty".into(),
        ));
    }
    let mut url = url::Url::parse(base).map_err(|e| {
        StorageError::Misconfigured(format!("public_base `{base}` is not an absolute URL: {e}"))
    })?;
    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            StorageError::Misconfigured(format!("public_base `{base}` cannot accept a path"))
        })?;
        segments.pop_if_empty();
        for component in normalized.components() {
            let std::path::Component::Normal(name) = component else {
                return Err(StorageError::BadRequest(
                    "storage key contains a disallowed path component".into(),
                ));
            };
            let name = name
                .to_str()
                .ok_or_else(|| StorageError::BadRequest("storage key is not valid UTF-8".into()))?;
            segments.push(name);
        }
    }
    Ok(url.to_string())
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
        let root = self.canonical_root()?;
        let prefix_path = self.resolve_prefix(prefix)?;
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
                        .strip_prefix(&root)
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
        let url = presigned_local_url(base, key)?;
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
        let url = presigned_local_url(base, key)?;
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
