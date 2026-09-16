//! Health probe decorators (`#[liveness]` / `#[readiness]` / `#[startup]`) +
//! the `install_probes` resolver that mounts them on the live router.
//!
//! # Decorators
//!
//! The decorators are route-metadata stamps (same pipeline as `#[throttle]`):
//! applying `#[liveness]` to a `#[get("/healthz")]` handler records
//! `probe => liveness` for that handler. At router-build time
//! [`install_probes`] resolves the stamped routes and mounts three fixed GET
//! endpoints at the server root (like `enable_health_check`, they are
//! unaffected by `set_global_prefix` / URI versioning):
//!
//! - `GET /__nestrs/health/live`    — mirrors the `#[liveness]` handler's
//!   status (2xx ⇒ up, anything else ⇒ 503). No stamp ⇒ always up.
//! - `GET /__nestrs/health/ready`   — mirrors the `#[readiness]` handler, or
//!   aggregates the readiness indicators when no handler is stamped.
//! - `GET /__nestrs/health/startup` — mirrors the `#[startup]` handler.
//!   Evaluated **once per process**; every later probe returns the first
//!   result.
//!
//! Mirroring is implemented as an internal self-request through the completed
//! router, so the stamped handler runs with its real extractors, guards, and
//! middleware. If the stamped path is itself unreachable (guard rejects, route
//! errors), the probe reports down — stamp a route that is reachable by
//! unauthenticated k8s probes.
//!
//! # Hardening
//!
//! The probe endpoints mount **outside** every middleware layer (they must
//! stay reachable under load shedding and rate limits), which makes them an
//! unauthenticated surface. Three protections apply as a result:
//!
//! - **Panic guard** — probe execution runs on its own task; a panicking
//!   indicator or stamped handler becomes a 503, never a dropped connection.
//! - **Generic Down messages** — responses say *that* the check failed (plus
//!   failing indicator names), never *why*: raw error strings (DB endpoints,
//!   dependency URLs, paths) go to `tracing` only.
//! - **Short-TTL cache with in-flight coalescing** — liveness and readiness
//!   outcomes are cached for 5s and concurrent probes share one execution,
//!   so a probe storm (or an attacker) cannot amplify work into the
//!   dependencies the indicators call. This is deliberately *not* a 429-style
//!   rate cap: k8s treats any non-2xx probe as a failure and restarts the pod,
//!   so rate-limiting a probe would fail the probe.

use crate::{HealthIndicator, HealthStatus};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use nestrs_core::MetadataRegistry;
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
static PROBE_ROUTES: OnceLock<RwLock<HashMap<ProbeKind, (&'static str, String)>>> =
    OnceLock::new();

fn probe_routes() -> &'static RwLock<HashMap<ProbeKind, (&'static str, String)>> {
    PROBE_ROUTES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// In-process startup-probe cache: the stamped `#[startup]` handler runs at
/// most once per process, whichever app hits it first.
static STARTUP_CACHE: tokio::sync::OnceCell<ProbeOutcome> = tokio::sync::OnceCell::const_new();

/// How long a liveness/readiness outcome stays fresh. k8s default
/// `periodSeconds` is 10, so a 5s cache never hides state for more than half a
/// probe period — but it caps indicator execution (DB pings, dependency HTTP
/// GETs) at one run per window no matter how fast the endpoint is hammered.
const PROBE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// Generic Down message served when a probe execution panics. The panic
/// detail goes to `tracing`.
const PROBE_PANIC_MESSAGE: &str = "probe execution failed";

/// Outcome cache for one probe kind: fresh results are served directly,
/// concurrent callers share the in-flight execution, and a completed result
/// is served from cache until the TTL lapses. Execution rate is thereby
/// capped at one run per TTL window per probe kind.
#[derive(Clone)]
struct ProbeCache {
    inner: Arc<tokio::sync::Mutex<Option<(std::time::Instant, ProbeOutcome)>>>,
}

impl ProbeCache {
    fn new() -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Serve the cached outcome when fresh; otherwise start `make` (which
    /// spawns the guarded execution) and store its result. Callers arriving
    /// while an execution is in flight wait for it instead of piling on —
    /// one execution serves the whole concurrent burst.
    async fn get_or_refresh(
        &self,
        make: impl FnOnce() -> tokio::task::JoinHandle<ProbeOutcome>,
    ) -> ProbeOutcome {
        let mut guard = self.inner.lock().await;
        if let Some((at, ref outcome)) = *guard {
            if at.elapsed() < PROBE_CACHE_TTL {
                return outcome.clone();
            }
        }
        // Probe execution runs on its own task: a panic in an indicator or
        // stamped handler surfaces as a JoinError here (→ 503) instead of
        // unwinding the connection's task and dropping the probe request.
        let outcome = match make().await {
            Ok(outcome) => outcome,
            Err(join_err) => {
                tracing::error!(
                    target: "nestrs",
                    "health probe execution panicked: {join_err}"
                );
                ProbeOutcome::Down {
                    status: 500,
                    message: PROBE_PANIC_MESSAGE.to_string(),
                }
            }
        };
        *guard = Some((std::time::Instant::now(), outcome.clone()));
        outcome
    }
}

/// Outcome of a mirrored probe handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Up,
    Down { status: u16, message: String },
}

impl From<ProbeOutcome> for HealthStatus {
    fn from(o: ProbeOutcome) -> Self {
        match o {
            ProbeOutcome::Up => Self::Up,
            ProbeOutcome::Down { message, .. } => Self::down(message),
        }
    }
}

/// Issue an internal self-request through the captured main router and
/// translate the response status into a probe outcome. The Down message is
/// generic (`"{kind} check failed"`) — the method/path/status detail goes to
/// `tracing` only, since the probe endpoints are reachable without
/// authentication and must not disclose internal route topology.
async fn mirror_request(
    router: axum::Router,
    method: &'static str,
    path: &str,
    kind: &'static str,
) -> ProbeOutcome {
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
                tracing::warn!(
                    target: "nestrs",
                    "{kind} probe: stamped handler {method} {path} returned {status}"
                );
                ProbeOutcome::Down {
                    status,
                    message: format!("{kind} check failed"),
                }
            }
        }
        Err(err) => {
            tracing::error!(
                target: "nestrs",
                "{kind} probe: dispatch to {method} {path} failed: {err}"
            );
            ProbeOutcome::Down {
                status: 500,
                message: format!("{kind} check failed"),
            }
        }
    }
}

/// Aggregate the readiness indicators into one outcome. Failing indicator
/// *names* ride in the message (they are the operator's own static labels);
/// their raw error text goes to `tracing` only.
async fn aggregate_readiness(indicators: Vec<Arc<dyn HealthIndicator>>) -> ProbeOutcome {
    let mut failing: Vec<&str> = Vec::new();
    for ind in &indicators {
        if let HealthStatus::Down { message } = ind.check().await {
            tracing::warn!(
                target: "nestrs",
                "readiness indicator '{}' reports down: {message}",
                ind.name()
            );
            failing.push(ind.name());
        }
    }
    if failing.is_empty() {
        ProbeOutcome::Up
    } else {
        ProbeOutcome::Down {
            status: 503,
            message: format!("readiness failed ({})", failing.join("; ")),
        }
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
/// Called from `build_router` once the main router is complete: `router` is
/// captured for mirror dispatch and the three fixed endpoints are mounted on
/// top. An endpoint is only mounted when nothing else already claims its path
/// (the user's own `enable_liveness_check` / `enable_readiness_check` routes
/// win).
pub fn install_probes(
    router: axum::Router,
    readiness_indicators: Vec<Arc<dyn HealthIndicator>>,
) -> axum::Router {
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
        if nestrs_core::RouteRegistry::handler_for("GET", kind.path()).is_some() {
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
                            // Startup is evaluated once per process (k8s
                            // contract) — the OnceCell is the cap.
                            let outcome = match &stamped_route {
                                Some((method, path)) => {
                                    let (method, path) = (*method, path.clone());
                                    STARTUP_CACHE
                                        .get_or_init(|| async {
                                            mirror_request(
                                                (*main).clone(),
                                                method,
                                                &path,
                                                "startup",
                                            )
                                            .await
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
                let cache = ProbeCache::new();
                probe_router = probe_router.route(
                    kind.path(),
                    axum::routing::get(move || {
                        let main = std::sync::Arc::clone(&main);
                        let cache = cache.clone();
                        async move {
                            let outcome = match stamped_route.clone() {
                                Some((method, path)) => {
                                    let main = std::sync::Arc::clone(&main);
                                    cache
                                        .get_or_refresh(move || {
                                            tokio::spawn(async move {
                                                mirror_request(
                                                    (*main).clone(),
                                                    method,
                                                    &path,
                                                    "liveness",
                                                )
                                                .await
                                            })
                                        })
                                        .await
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
                let cache = ProbeCache::new();
                let indicators: Vec<Arc<dyn HealthIndicator>> =
                    readiness_indicators.iter().map(Arc::clone).collect();
                probe_router = probe_router.route(
                    kind.path(),
                    axum::routing::get(move || {
                        let main = std::sync::Arc::clone(&main);
                        let cache = cache.clone();
                        let indicators = indicators.clone();
                        async move {
                            let outcome = match stamped_route.clone() {
                                Some((method, path)) => {
                                    let main = std::sync::Arc::clone(&main);
                                    cache
                                        .get_or_refresh(move || {
                                            tokio::spawn(async move {
                                                mirror_request(
                                                    (*main).clone(),
                                                    method,
                                                    &path,
                                                    "readiness",
                                                )
                                                .await
                                            })
                                        })
                                        .await
                                }
                                None => {
                                    // No stamped handler: aggregate the
                                    // readiness indicators.
                                    cache
                                        .get_or_refresh(move || {
                                            tokio::spawn(async move {
                                                aggregate_readiness(indicators).await
                                            })
                                        })
                                        .await
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

/// Resolve the stamped probe routes from the global route + metadata
/// registries. Called from `build_router` before the endpoints are mounted.
fn resolve_stamped_probes() {
    let mut map = probe_routes().write().expect("probe route lock poisoned");
    for route in nestrs_core::RouteRegistry::list() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_kind_paths_are_distinct() {
        let paths: Vec<&str> = ProbeKind::all().iter().map(|k| k.path()).collect();
        let unique: std::collections::HashSet<&str> = paths.iter().copied().collect();
        assert_eq!(paths.len(), unique.len(), "probe paths must be unique");
        assert!(paths.contains(&"/__nestrs/health/live"));
        assert!(paths.contains(&"/__nestrs/health/ready"));
        assert!(paths.contains(&"/__nestrs/health/startup"));
    }

    #[test]
    fn probe_kind_round_trip_parse() {
        for kind in ProbeKind::all() {
            assert_eq!(ProbeKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ProbeKind::parse("nope"), None);
    }
}
