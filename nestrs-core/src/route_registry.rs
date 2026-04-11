use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RouteInfo {
    pub method: &'static str,
    pub path: &'static str,
    pub handler: &'static str,
}

fn store() -> &'static RwLock<Vec<RouteInfo>> {
    static STORE: OnceLock<RwLock<Vec<RouteInfo>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(Vec::new()))
}

/// Global route registry (used by OpenAPI generation and diagnostics).
pub struct RouteRegistry;

impl RouteRegistry {
    pub fn register(method: &'static str, path: &'static str, handler: &'static str) {
        let mut guard = store().write().expect("route registry lock poisoned");
        guard.push(RouteInfo {
            method,
            path,
            handler,
        });
    }

    pub fn list() -> Vec<RouteInfo> {
        let guard = store().read().expect("route registry lock poisoned");
        guard.clone()
    }
}

