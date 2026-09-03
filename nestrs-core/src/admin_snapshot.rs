//! `AdminSnapshot` — a serializable view of the framework's three global
//! registries (provider, route, metadata), shaped for HTTP responses
//! served by the `nestrs::admin` sidecar port and consumed by
//! `nestrs-mcp`'s live runtime tools.
//!
//! Always available — no feature gate. The HTTP surface in `nestrs::admin`
//! is what costs the `axum` dep, and that's gated separately.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use crate::route_registry::RouteInfo;
use crate::ProviderSummary;

/// A snapshot of the live registries at a point in time. Cloned cheaply
/// via `Arc` so the admin handler can serve many requests without
/// re-reading the route registry under lock each time.
#[derive(Clone, Debug)]
pub struct AdminSnapshot {
    pub providers: Vec<ProviderSummary>,
    pub routes: Vec<RouteInfo>,
    /// Map of handler key → (metadata key → metadata value).
    pub metadata: BTreeMap<String, BTreeMap<String, String>>,
    /// Process uptime in milliseconds, since the first `snapshot()` call.
    pub uptime_ms: u128,
    /// `CARGO_PKG_VERSION` of the binary hosting the admin port.
    pub version: &'static str,
}

impl AdminSnapshot {
    /// Build a snapshot from the current registry state, with the
    /// provider list supplied by the caller (since the sidecar is the one
    /// that holds the `Arc<ProviderRegistry>`).
    pub fn capture(providers: Vec<ProviderSummary>, version: &'static str) -> Arc<Self> {
        let routes = crate::route_registry::RouteRegistry::list();
        let metadata = crate::metadata::MetadataRegistry::snapshot();
        Arc::new(Self {
            providers,
            routes,
            metadata,
            uptime_ms: process_uptime_ms(),
            version,
        })
    }
}

fn process_uptime_ms() -> u128 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_millis()
}
