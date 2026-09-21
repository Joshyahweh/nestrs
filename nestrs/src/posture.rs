//! Route **posture** — every HTTP route must declare `#[public]` or carry
//! `#[use_guards(...)]` when [`crate::NestApplication::require_route_posture`]
//! is enabled.
//!
//! Metadata key: `nestrs.posture` → `public` | `guarded` (written by
//! `nestrs-macros` at route registration).

use crate::core::{MetadataRegistry, RouteRegistry};

/// Metadata key written by `#[routes]` / `#[public]` / `#[use_guards]`.
pub const POSTURE_METADATA_KEY: &str = "nestrs.posture";

/// Error listing routes that lack an access posture.
#[derive(Debug, Clone)]
pub struct PostureError {
    pub unguarded: Vec<UnguardedRoute>,
}

impl std::fmt::Display for PostureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unguarded routes detected ({}). Mark each with #[public] or #[use_guards(...)] \
             (or disable NestApplication::require_route_posture): ",
            self.unguarded.len()
        )?;
        for (i, r) in self.unguarded.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{} {}", r.method, r.path)?;
        }
        Ok(())
    }
}

impl std::error::Error for PostureError {}

/// One route missing posture metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnguardedRoute {
    pub method: String,
    pub path: String,
    pub handler: String,
}

fn should_exclude(path: &str, exclude_prefixes: &[&str]) -> bool {
    exclude_prefixes.iter().any(|p| {
        if p.is_empty() || *p == "/" {
            return false;
        }
        path == *p || path.starts_with(&format!("{p}/"))
    })
}

/// Scan [`RouteRegistry`] + [`MetadataRegistry`] for routes without posture.
///
/// Paths that equal or sit under any entry in `exclude_prefixes` are skipped
/// (health, metrics, OpenAPI, admin).
pub fn collect_unguarded_routes(exclude_prefixes: &[&str]) -> Vec<UnguardedRoute> {
    let mut out = Vec::new();
    for route in RouteRegistry::list() {
        if should_exclude(route.path, exclude_prefixes) {
            continue;
        }
        match MetadataRegistry::get(route.handler, POSTURE_METADATA_KEY).as_deref() {
            Some("public") | Some("guarded") => {}
            _ => out.push(UnguardedRoute {
                method: route.method.to_string(),
                path: route.path.to_string(),
                handler: route.handler.to_string(),
            }),
        }
    }
    out
}

/// Fail if any application route lacks posture.
pub fn assert_route_posture(exclude_prefixes: &[&str]) -> Result<(), PostureError> {
    let unguarded = collect_unguarded_routes(exclude_prefixes);
    if unguarded.is_empty() {
        Ok(())
    } else {
        Err(PostureError { unguarded })
    }
}

#[cfg(all(test, feature = "test-hooks"))]
mod tests {
    use super::*;
    use crate::core::{MetadataRegistry, RouteRegistry};

    #[test]
    fn detects_unguarded_and_respects_excludes() {
        RouteRegistry::clear_for_tests();
        MetadataRegistry::clear_for_tests();
        RouteRegistry::register("GET", "/api/items", "demo::list");
        RouteRegistry::register("GET", "/health", "demo::health");
        MetadataRegistry::set("demo::health", POSTURE_METADATA_KEY, "public");
        let bad = collect_unguarded_routes(&["/health"]);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].path, "/api/items");
        MetadataRegistry::set("demo::list", POSTURE_METADATA_KEY, "guarded");
        assert!(assert_route_posture(&["/health"]).is_ok());
    }
}
