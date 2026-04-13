use std::sync::{OnceLock, RwLock};

/// One HTTP response line for OpenAPI generation (per route).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OpenApiResponseDesc {
    pub status: u16,
    pub description: &'static str,
}

/// Optional per-route OpenAPI metadata (from `#[openapi(...)]` / `impl_routes!` `openapi` clause).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OpenApiRouteSpec {
    pub summary: Option<&'static str>,
    pub tag: Option<&'static str>,
    pub responses: &'static [OpenApiResponseDesc],
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteInfo {
    pub method: &'static str,
    pub path: &'static str,
    pub handler: &'static str,
    pub openapi: Option<&'static OpenApiRouteSpec>,
}

fn store() -> &'static RwLock<Vec<RouteInfo>> {
    static STORE: OnceLock<RwLock<Vec<RouteInfo>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(Vec::new()))
}

/// Global route registry (used by OpenAPI generation and diagnostics).
pub struct RouteRegistry;

impl RouteRegistry {
    /// Register a route with no OpenAPI overrides (defaults: inferred summary/tags, `200` only).
    pub fn register(method: &'static str, path: &'static str, handler: &'static str) {
        Self::register_spec(method, path, handler, None);
    }

    /// Register a route; `openapi` may point at a leaked or `const` [`OpenApiRouteSpec`].
    pub fn register_spec(
        method: &'static str,
        path: &'static str,
        handler: &'static str,
        openapi: Option<&'static OpenApiRouteSpec>,
    ) {
        let mut guard = store().write().expect("route registry lock poisoned");
        guard.push(RouteInfo {
            method,
            path,
            handler,
            openapi,
        });
    }

    pub fn list() -> Vec<RouteInfo> {
        let guard = store().read().expect("route registry lock poisoned");
        guard.clone()
    }

    /// Clears all registered HTTP routes in this process.
    ///
    /// **Available only with the `test-hooks` feature.** Intended for integration tests that share
    /// a process with other tests; production applications must never call this (routes would
    /// disappear from OpenAPI / diagnostics).
    #[cfg(feature = "test-hooks")]
    pub fn clear_for_tests() {
        let mut guard = store().write().expect("route registry lock poisoned");
        guard.clear();
    }
}
