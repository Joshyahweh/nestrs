//! Health probe decorators (`#[liveness]` / `#[readiness]` / `#[startup]`)
//! plus the standard indicator set (NestJS terminus).
//!
//! # Decorators
//!
//! The decorators are route-metadata stamps (same pipeline as
//! `#[throttle]`): applying `#[liveness]` to a `#[get("/healthz")]`
//! handler records `probe => liveness` for that handler. At router-build
//! time [`install_probes`] resolves the stamped routes and mounts three
//! fixed GET endpoints at the server root (like `enable_health_check`,
//! they are unaffected by `set_global_prefix` / URI versioning):
//!
//! - `GET /__nestrs/health/live`    — mirrors the `#[liveness]` handler's
//!   status (2xx ⇒ up, anything else ⇒ 503). No stamp ⇒ always up.
//! - `GET /__nestrs/health/ready`   — mirrors the `#[readiness]` handler,
//!   or aggregates the `enable_readiness_check` indicators when no
//!   handler is stamped.
//! - `GET /__nestrs/health/startup` — mirrors the `#[startup]` handler.
//!   Evaluated **once per process**; every later probe returns the first
//!   result.
//!
//! Mirroring is implemented as an internal self-request through the
//! completed router, so the stamped handler runs with its real
//! extractors, guards, and middleware. If the stamped path is itself
//! unreachable (guard rejects, route errors), the probe reports down —
//! stamp a route that is reachable by unauthenticated k8s probes.
//!
//! # Indicators
//!
//! [`DatabaseIndicator`], [`HttpIndicator`], and [`DiskSpaceIndicator`]
//! implement the existing [`crate::HealthIndicator`] trait, so they drop
//! straight into `enable_readiness_check` (and are also used as the
//! default readiness source when no `#[readiness]` handler is stamped).
//! `HttpIndicator` requires the `http-client` feature; `DiskSpaceIndicator`
//! requires `health-disk` (unix only).

use crate::core::MetadataRegistry;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

/// Which probe a `#[liveness]` / `#[readiness]` / `#[startup]` stamp marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProbeKind {
    Liveness,
    Readiness,
    Startup,
}

impl ProbeKind {
    /// The metadata value emitted by the decorator.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Liveness => "liveness",
            Self::Readiness => "readiness",
            Self::Startup => "startup",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "liveness" => Some(Self::Liveness),
            "readiness" => Some(Self::Readiness),
            "startup" => Some(Self::Startup),
            _ => None,
        }
    }

    /// The fixed server-root path serving this probe.
    pub fn path(self) -> &'static str {
        match self {
            Self::Liveness => "/__nestrs/health/live",
            Self::Readiness => "/__nestrs/health/ready",
            Self::Startup => "/__nestrs/health/startup",
        }
    }

    fn all() -> [Self; 3] {
        [Self::Liveness, Self::Readiness, Self::Startup]
    }
}

/// `(method, path)` of the handler stamped for each probe kind, resolved
/// from the route registry at `build_router` time.
static PROBE_ROUTES: OnceLock<RwLock<HashMap<ProbeKind, (&'static str, String)>>> = OnceLock::new();

fn probe_routes() -> &'static RwLock<HashMap<ProbeKind, (&'static str, String)>> {
    PROBE_ROUTES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// In-process startup-probe cache: the stamped `#[startup]` handler runs at
/// most once per process, whichever app hits it first.
static STARTUP_CACHE: tokio::sync::OnceCell<ProbeOutcome> = tokio::sync::OnceCell::const_new();

/// Resolve the stamped probe routes from the global route + metadata
/// registries. Called from `build_router` before the endpoints are mounted.
fn resolve_stamped_probes() {
    let mut map = probe_routes().write().expect("probe route lock poisoned");
    for route in crate::core::RouteRegistry::list() {
        let Some(kind) = MetadataRegistry::get(route.handler, "probe")
            .as_deref()
            .and_then(ProbeKind::parse)
        else {
            continue;
        };
        // Recursion guard: a handler stamped onto one of the probe
        // endpoints themselves would self-request forever.
        if ProbeKind::all().iter().any(|k| k.path() == route.path) {
            continue;
        }
        map.insert(kind, (route.method, route.path.to_string()));
    }
}

/// Outcome of a mirrored probe handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Up,
    Down { status: u16, message: String },
}

impl From<ProbeOutcome> for crate::HealthStatus {
    fn from(o: ProbeOutcome) -> Self {
        match o {
            ProbeOutcome::Up => Self::Up,
            ProbeOutcome::Down { message, .. } => Self::down(message),
        }
    }
}

/// Issue an internal self-request through the captured main router and
/// translate the response status into a probe outcome.
async fn mirror_request(router: axum::Router, method: &'static str, path: &str) -> ProbeOutcome {
    use tower::ServiceExt;
    let request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .body(axum::body::Body::empty())
        .expect("static request parts");
    match router.oneshot(request).await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if resp.status().is_success() {
                ProbeOutcome::Up
            } else {
                ProbeOutcome::Down {
                    status,
                    message: format!("probe handler {method} {path} returned {status}"),
                }
            }
        }
        Err(err) => ProbeOutcome::Down {
            status: 500,
            message: format!("probe dispatch to {method} {path} failed: {err}"),
        },
    }
}

fn outcome_response(outcome: &ProbeOutcome) -> Response {
    match outcome {
        ProbeOutcome::Up => (
            StatusCode::OK,
            axum::Json(serde_json::json!({ "status": "ok" })),
        )
            .into_response(),
        ProbeOutcome::Down { message, .. } => {
            // Always 503: the canonical "not ready" for k8s probes / load
            // balancers, whatever the underlying failure. The mirrored
            // handler's own status rides in the message.
            (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({
                    "status": "error",
                    "message": message,
                })),
            )
                .into_response()
        }
    }
}

/// Install the probe endpoints and return them merged **in front of**
/// `router`.
///
/// Called from `build_router` once the main router is complete: `router`
/// is captured for mirror dispatch and the three fixed endpoints are
/// mounted on top. An endpoint is only mounted when nothing else already
/// claims its path (the user's own `enable_liveness_check` /
/// `enable_readiness_check` routes win).
pub fn install_probes(
    router: axum::Router,
    readiness_indicators: Vec<Arc<dyn crate::HealthIndicator>>,
) -> axum::Router {
    use crate::HealthStatus;
    resolve_stamped_probes();
    let main = std::sync::Arc::new(router.clone());

    let stamped = probe_routes()
        .read()
        .expect("probe route lock poisoned")
        .clone();

    let mut probe_router = axum::Router::new();
    for kind in ProbeKind::all() {
        let stamped_route = stamped.get(&kind).map(|(m, p)| (*m, p.clone()));
        // The user's own route (or the method-based liveness/readiness
        // endpoints) already owns this path — leave it alone.
        if crate::core::RouteRegistry::handler_for("GET", kind.path()).is_some() {
            continue;
        }

        match kind {
            ProbeKind::Startup => {
                let main = std::sync::Arc::clone(&main);
                probe_router = probe_router.route(
                    kind.path(),
                    axum::routing::get(move || {
                        let main = std::sync::Arc::clone(&main);
                        async move {
                            let outcome = match &stamped_route {
                                Some((method, path)) => {
                                    let (method, path) = (*method, path.clone());
                                    STARTUP_CACHE
                                        .get_or_init(|| async {
                                            mirror_request((*main).clone(), method, &path).await
                                        })
                                        .await
                                }
                                None => {
                                    STARTUP_CACHE
                                        .get_or_init(|| async { ProbeOutcome::Up })
                                        .await
                                }
                            };
                            outcome_response(outcome)
                        }
                    }),
                );
            }
            ProbeKind::Liveness => {
                let main = std::sync::Arc::clone(&main);
                probe_router = probe_router.route(
                    kind.path(),
                    axum::routing::get(move || {
                        let main = std::sync::Arc::clone(&main);
                        async move {
                            let outcome = match &stamped_route {
                                Some((method, path)) => {
                                    mirror_request((*main).clone(), method, path).await
                                }
                                None => ProbeOutcome::Up,
                            };
                            outcome_response(&outcome)
                        }
                    }),
                );
            }
            ProbeKind::Readiness => {
                let main = std::sync::Arc::clone(&main);
                let indicators: Vec<Arc<dyn crate::HealthIndicator>> =
                    readiness_indicators.iter().map(Arc::clone).collect();
                probe_router = probe_router.route(
                    kind.path(),
                    axum::routing::get(move || {
                        let main = std::sync::Arc::clone(&main);
                        let indicators = indicators.clone();
                        async move {
                            let outcome = if let Some((method, path)) = &stamped_route {
                                mirror_request((*main).clone(), method, path).await
                            } else {
                                // No stamped handler: aggregate the
                                // `enable_readiness_check` indicators.
                                let mut downs: Vec<String> = Vec::new();
                                for ind in &indicators {
                                    if let HealthStatus::Down { message } = ind.check().await {
                                        downs.push(format!("{}: {message}", ind.name()));
                                    }
                                }
                                if downs.is_empty() {
                                    ProbeOutcome::Up
                                } else {
                                    ProbeOutcome::Down {
                                        status: 503,
                                        message: downs.join("; "),
                                    }
                                }
                            };
                            outcome_response(&outcome)
                        }
                    }),
                );
            }
        }
    }

    axum::Router::new().merge(probe_router).merge(router)
}

// ---------------------------------------------------------------------------
// Standard indicators
// ---------------------------------------------------------------------------

/// Readiness indicator over the shared [`crate::core::DatabasePing`]
/// capability (implemented by the SQLx / Prisma / Mongo services). No
/// feature gate — the trait lives in `nestrs-core`.
pub struct DatabaseIndicator {
    db: Arc<dyn crate::core::DatabasePing>,
}

impl DatabaseIndicator {
    pub fn new(db: Arc<dyn crate::core::DatabasePing>) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl crate::HealthIndicator for DatabaseIndicator {
    fn name(&self) -> &'static str {
        "database"
    }

    async fn check(&self) -> crate::HealthStatus {
        match self.db.ping_database().await {
            Ok(()) => crate::HealthStatus::Up,
            Err(e) => crate::HealthStatus::down(e),
        }
    }
}

/// Readiness indicator that GETs a dependency URL and requires a 2xx
/// within `timeout`. Requires the **`http-client`** feature.
#[cfg(feature = "http-client")]
pub struct HttpIndicator {
    url: String,
    timeout: std::time::Duration,
    client: reqwest::Client,
}

#[cfg(feature = "http-client")]
impl HttpIndicator {
    pub fn new(url: impl Into<String>, timeout: std::time::Duration) -> Self {
        Self {
            url: url.into(),
            timeout,
            client: reqwest::Client::new(),
        }
    }
}

#[cfg(feature = "http-client")]
#[async_trait::async_trait]
impl crate::HealthIndicator for HttpIndicator {
    fn name(&self) -> &'static str {
        "http"
    }

    async fn check(&self) -> crate::HealthStatus {
        match tokio::time::timeout(self.timeout, self.client.get(&self.url).send()).await {
            Ok(Ok(resp)) if resp.status().is_success() => crate::HealthStatus::Up,
            Ok(Ok(resp)) => crate::HealthStatus::down(format!(
                "dependency {} returned {}",
                self.url,
                resp.status()
            )),
            Ok(Err(e)) => {
                crate::HealthStatus::down(format!("dependency {} unreachable: {e}", self.url))
            }
            Err(_) => crate::HealthStatus::down(format!(
                "dependency {} timed out after {:?}",
                self.url, self.timeout
            )),
        }
    }
}

/// Readiness indicator that reports Down when the free space on the
/// filesystem holding `path` drops below `min_free_bytes`. Requires the
/// **`health-disk`** feature (unix only — backed by `statvfs`).
#[cfg(all(unix, feature = "health-disk"))]
pub struct DiskSpaceIndicator {
    path: std::path::PathBuf,
    min_free_bytes: u64,
}

#[cfg(all(unix, feature = "health-disk"))]
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

#[cfg(all(unix, feature = "health-disk"))]
#[async_trait::async_trait]
impl crate::HealthIndicator for DiskSpaceIndicator {
    fn name(&self) -> &'static str {
        "disk"
    }

    async fn check(&self) -> crate::HealthStatus {
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
            Ok(free) if free >= min_free_bytes => crate::HealthStatus::Up,
            Ok(free) => crate::HealthStatus::down(format!(
                "free space on {:?} is {free} bytes (needs >= {})",
                path, min_free_bytes
            )),
            Err(e) => crate::HealthStatus::down(format!("statvfs({:?}) failed: {e}", path)),
        }
    }
}
