//! Health indicator trait + standard indicators (`DatabaseIndicator`,
//! `HttpIndicator`, `DiskSpaceIndicator`).

/// Result of a single [`HealthIndicator::check`].
#[derive(Debug, Clone)]
pub enum HealthStatus {
    Up,
    Down { message: String },
}

impl HealthStatus {
    pub fn down(message: impl Into<String>) -> Self {
        Self::Down {
            message: message.into(),
        }
    }
}

/// Pluggable readiness check (database ping, broker, external HTTP, etc.).
#[async_trait::async_trait]
pub trait HealthIndicator: Send + Sync {
    fn name(&self) -> &'static str;

    async fn check(&self) -> HealthStatus;
}

/// Holds indicators for [`crate::install_probes`]'s default readiness source
/// and for `NestApplication::enable_readiness_check`; exposed so apps can
/// reuse or test checks.
#[derive(Clone)]
pub struct ReadinessContext {
    pub indicators: Vec<std::sync::Arc<dyn HealthIndicator>>,
}

impl ReadinessContext {
    pub fn new(indicators: Vec<std::sync::Arc<dyn HealthIndicator>>) -> Self {
        Self { indicators }
    }

    pub fn indicators(&self) -> &[std::sync::Arc<dyn HealthIndicator>] {
        &self.indicators
    }
}

// ---------------------------------------------------------------------------
// Standard indicators
// ---------------------------------------------------------------------------

/// Readiness indicator over the shared [`nestrs_core::DatabasePing`]
/// capability (implemented by the SQLx / Prisma / Mongo services). No
/// feature gate — the trait lives in `nestrs-core`.
pub struct DatabaseIndicator {
    db: std::sync::Arc<dyn nestrs_core::DatabasePing>,
}

impl DatabaseIndicator {
    pub fn new(db: std::sync::Arc<dyn nestrs_core::DatabasePing>) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl HealthIndicator for DatabaseIndicator {
    fn name(&self) -> &'static str {
        "database"
    }

    async fn check(&self) -> HealthStatus {
        match self.db.ping_database().await {
            Ok(()) => HealthStatus::Up,
            Err(e) => HealthStatus::down(e),
        }
    }
}

/// Readiness indicator that GETs a dependency URL and requires a 2xx
/// within `timeout`. Requires the **`http`** feature.
#[cfg(feature = "http")]
pub struct HttpIndicator {
    url: String,
    timeout: std::time::Duration,
    client: reqwest::Client,
}

#[cfg(feature = "http")]
impl HttpIndicator {
    pub fn new(url: impl Into<String>, timeout: std::time::Duration) -> Self {
        Self {
            url: url.into(),
            timeout,
            client: reqwest::Client::new(),
        }
    }
}

#[cfg(feature = "http")]
#[async_trait::async_trait]
impl HealthIndicator for HttpIndicator {
    fn name(&self) -> &'static str {
        "http"
    }

    async fn check(&self) -> HealthStatus {
        match tokio::time::timeout(self.timeout, self.client.get(&self.url).send()).await {
            Ok(Ok(resp)) if resp.status().is_success() => HealthStatus::Up,
            Ok(Ok(resp)) => HealthStatus::down(format!(
                "dependency {} returned {}",
                self.url,
                resp.status()
            )),
            Ok(Err(e)) => {
                HealthStatus::down(format!("dependency {} unreachable: {e}", self.url))
            }
            Err(_) => HealthStatus::down(format!(
                "dependency {} timed out after {:?}",
                self.url, self.timeout
            )),
        }
    }
}

/// Readiness indicator that reports Down when the free space on the
/// filesystem holding `path` drops below `min_free_bytes`. Requires the
/// **`disk`** feature (unix only — backed by `statvfs`).
#[cfg(all(unix, feature = "disk"))]
pub struct DiskSpaceIndicator {
    path: std::path::PathBuf,
    min_free_bytes: u64,
}

#[cfg(all(unix, feature = "disk"))]
impl DiskSpaceIndicator {
    pub fn new(path: impl Into<std::path::PathBuf>, min_free_bytes: u64) -> Self {
        Self {
            path: path.into(),
            min_free_bytes,
        }
    }

    /// Free bytes for an already-cloned path. Used from the blocking task so
    /// the closure doesn't need to borrow `&self`.
    fn free_bytes_for(path: std::path::PathBuf) -> Result<u64, String> {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|e| format!("invalid path {:?}: {e}", path))?;
        // SAFETY: `c_path` outlives the call and `statvfs` only reads it;
        // `vfs` is a valid, initialized `statvfs` destination.
        let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut vfs) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // `f_bavail` is `u32` on macOS, `u64` on Linux; widen so the result
        // is always `u64` regardless of platform.
        let bavail = vfs.f_bavail as u64;
        let frsize = vfs.f_frsize as u64;
        Ok(bavail.saturating_mul(frsize))
    }
}

#[cfg(all(unix, feature = "disk"))]
#[async_trait::async_trait]
impl HealthIndicator for DiskSpaceIndicator {
    fn name(&self) -> &'static str {
        "disk"
    }

    async fn check(&self) -> HealthStatus {
        // statvfs can block on large mounts; run it off the async workers.
        // Capture the path + threshold up-front so the closure owns only the
        // values it needs and we don't borrow after the move.
        let path = self.path.clone();
        let min_free_bytes = self.min_free_bytes;
        let res = tokio::task::spawn_blocking({
            let path = path.clone();
            move || Self::free_bytes_for(path)
        })
        .await
        .expect("disk indicator task");
        match res {
            Ok(free) if free >= min_free_bytes => HealthStatus::Up,
            Ok(free) => HealthStatus::down(format!(
                "free space on {:?} is {free} bytes (needs >= {})",
                path, min_free_bytes
            )),
            Err(e) => HealthStatus::down(format!("statvfs({:?}) failed: {e}", path)),
        }
    }
}
