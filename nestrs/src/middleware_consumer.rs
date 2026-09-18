//! NestJS [`MiddlewareConsumer`](https://docs.nestjs.com/middleware) analogue:
//! apply a Tower/Axum middleware function to selected path prefixes, with excludes.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

type MiddlewareBox =
    Arc<dyn Fn(Request, Next) -> Pin<Box<dyn Future<Output = Response> + Send>> + Send + Sync>;

/// One compiled `apply` / `forRoutes` / `exclude` triple.
#[derive(Clone)]
pub struct MiddlewareRule {
    middleware: MiddlewareBox,
    /// Path prefixes to match (empty = every path). Compared with `starts_with`.
    include: Vec<String>,
    /// Path prefixes that skip this middleware even when included.
    exclude: Vec<String>,
}

impl MiddlewareRule {
    fn matches(&self, path: &str) -> bool {
        let included =
            self.include.is_empty() || self.include.iter().any(|prefix| path_matches(path, prefix));
        if !included {
            return false;
        }
        !self.exclude.iter().any(|prefix| path_matches(path, prefix))
    }
}

fn path_matches(path: &str, prefix: &str) -> bool {
    let prefix = if prefix.is_empty() || prefix == "/" {
        return true;
    } else if prefix.starts_with('/') {
        prefix
    } else {
        // Nest `forRoutes('admin')` means `/admin`.
        return path == format!("/{prefix}") || path.starts_with(&format!("/{prefix}/"));
    };
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// Nest-style middleware consumer: `apply` then `for_routes` / `exclude`.
///
/// ```ignore
/// NestFactory::create::<AppModule>().configure_middleware(
///     MiddlewareConsumer::new()
///         .apply_fn(|req, next| async move {
///             next.run(req).await
///         })
///         .for_routes(["/admin"])
///         .exclude(["/admin/health"]),
/// );
/// ```
#[derive(Clone, Default)]
pub struct MiddlewareConsumer {
    rules: Vec<MiddlewareRule>,
    pending: Option<PendingApply>,
}

#[derive(Clone)]
struct PendingApply {
    middleware: MiddlewareBox,
    include: Vec<String>,
    exclude: Vec<String>,
}

impl MiddlewareConsumer {
    /// Empty consumer (no-op until [`Self::apply_fn`] is used).
    pub fn new() -> Self {
        Self::default()
    }

    /// Start (or chain) a middleware function. Call [`Self::for_routes`] /
    /// [`Self::exclude`] after this to scope it; a subsequent [`Self::apply_fn`]
    /// finalizes the previous rule against all routes if you never scoped it.
    pub fn apply_fn<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(Request, Next) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        self.flush_pending();
        let f = Arc::new(f);
        self.pending = Some(PendingApply {
            middleware: Arc::new(move |req, next| {
                let f = Arc::clone(&f);
                Box::pin(async move { f(req, next).await })
            }),
            include: Vec::new(),
            exclude: Vec::new(),
        });
        self
    }

    /// Restrict the current [`Self::apply_fn`] to these path prefixes
    /// (`/admin` matches `/admin` and `/admin/users`).
    pub fn for_routes<I, S>(mut self, routes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if let Some(pending) = self.pending.as_mut() {
            pending.include = routes.into_iter().map(Into::into).collect();
        }
        self
    }

    /// Skip the current middleware for these path prefixes.
    pub fn exclude<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if let Some(pending) = self.pending.as_mut() {
            pending.exclude = paths.into_iter().map(Into::into).collect();
        }
        self
    }

    fn flush_pending(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.rules.push(MiddlewareRule {
                middleware: pending.middleware,
                include: pending.include,
                exclude: pending.exclude,
            });
        }
    }

    pub(crate) fn into_rules(mut self) -> Vec<MiddlewareRule> {
        self.flush_pending();
        self.rules
    }
}

pub(crate) async fn route_middleware(
    axum::extract::State(rule): axum::extract::State<MiddlewareRule>,
    req: Request,
    next: Next,
) -> Response {
    if rule.matches(req.uri().path()) {
        (rule.middleware)(req, next).await
    } else {
        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::path_matches;

    #[test]
    fn prefix_match_does_not_eat_sibling_paths() {
        assert!(path_matches("/admin", "/admin"));
        assert!(path_matches("/admin/users", "/admin"));
        assert!(!path_matches("/admins", "/admin"));
        assert!(path_matches("/admin", "admin"));
        assert!(!path_matches("/other", "/admin"));
    }
}
