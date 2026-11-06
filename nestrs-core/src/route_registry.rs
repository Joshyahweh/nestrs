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
    /// `components.schemas` name auto-linked as the JSON request body `$ref`.
    pub request_body_schema: Option<&'static str>,
    /// `components.schemas` name auto-linked as the `200` response `$ref`.
    pub response_schema: Option<&'static str>,
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

    /// Exact-match lookup of the handler name for `(method, path)`.
    ///
    /// Used by app-level middleware (throttler, probe normalization) that
    /// cannot read the `HandlerKey` request extension (only the route-level
    /// guard layer inserts it) but still needs the route's metadata. Paths
    /// must match the registered form exactly — the controller prefix joined
    /// with the route path; trailing slashes are normalized upstream by
    /// tower-http's `normalize-path` when enabled.
    pub fn handler_for(method: &str, path: &str) -> Option<String> {
        let guard = store().read().expect("route registry lock poisoned");
        guard
            .iter()
            .find(|r| r.method.eq_ignore_ascii_case(method) && r.path == path)
            .map(|r| r.handler.to_string())
    }

    /// Detect duplicate `(method, path)` registrations and simple static/param shadows.
    ///
    /// NestJS 12 `routeConflictPolicy` analogue. Returns human-readable findings;
    /// callers decide warn vs error.
    pub fn conflict_report() -> RouteConflictReport {
        let routes = Self::list();
        let mut duplicates = Vec::new();
        let mut shadows = Vec::new();
        let mut seen: std::collections::HashMap<(String, String), &'static str> =
            std::collections::HashMap::new();
        for r in &routes {
            let key = (r.method.to_ascii_uppercase(), r.path.to_string());
            if let Some(prev) = seen.insert(key.clone(), r.handler) {
                duplicates.push(RouteConflict {
                    method: r.method,
                    path: r.path,
                    first_handler: prev,
                    second_handler: r.handler,
                    kind: RouteConflictKind::Duplicate,
                });
            }
        }
        // Shadow: a static segment registered after a param sibling under the same method.
        for (i, earlier) in routes.iter().enumerate() {
            for later in routes.iter().skip(i + 1) {
                if !earlier.method.eq_ignore_ascii_case(later.method) {
                    continue;
                }
                if paths_shadow(earlier.path, later.path) {
                    shadows.push(RouteConflict {
                        method: later.method,
                        path: later.path,
                        first_handler: earlier.handler,
                        second_handler: later.handler,
                        kind: RouteConflictKind::Shadow,
                    });
                }
            }
        }
        RouteConflictReport {
            duplicates,
            shadows,
        }
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

/// Kind of route registration conflict (NestJS 12 diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteConflictKind {
    Duplicate,
    Shadow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteConflict {
    pub method: &'static str,
    pub path: &'static str,
    pub first_handler: &'static str,
    pub second_handler: &'static str,
    pub kind: RouteConflictKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteConflictReport {
    pub duplicates: Vec<RouteConflict>,
    pub shadows: Vec<RouteConflict>,
}

impl RouteConflictReport {
    pub fn is_empty(&self) -> bool {
        self.duplicates.is_empty() && self.shadows.is_empty()
    }
}

fn paths_shadow(earlier: &str, later: &str) -> bool {
    let a: Vec<&str> = earlier.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = later.split('/').filter(|s| !s.is_empty()).collect();
    if a.len() != b.len() {
        return false;
    }
    let mut earlier_has_param = false;
    let mut later_is_more_specific = false;
    for (ea, lb) in a.iter().zip(b.iter()) {
        let ea_param = ea.starts_with(':') || (ea.starts_with('{') && ea.ends_with('}'));
        let lb_param = lb.starts_with(':') || (lb.starts_with('{') && lb.ends_with('}'));
        if ea_param {
            earlier_has_param = true;
            if !lb_param && *ea != *lb {
                later_is_more_specific = true;
            }
        } else if lb_param || *ea != *lb {
            // `else` means `!ea_param`, so a later param segment or a
            // mismatched literal cannot be shadowed by this earlier route.
            return false;
        }
    }
    earlier_has_param && later_is_more_specific
}
