//! Nest-style **interceptors** — wrap the request/response around the handler (`next.run(req)`).

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/// Around-advice: run logic before/after the inner pipeline by calling [`Next::run`].
///
/// Apply with the [`crate::interceptor_layer`] macro on a [`axum::Router`] or via
/// [`crate::NestApplication::use_global_layer`]. Interceptors declared through `#[use_interceptors(...)]`
/// / `impl_routes!` are resolved **once at route registration** via [`Self::resolve`], so they may hold
/// DI dependencies; the standalone [`crate::interceptor_layer`] macro keeps using [`Default`].
#[async_trait::async_trait]
pub trait Interceptor: Default + Send + Sync + 'static {
    /// Build the interceptor instance used for every request on routes declaring it.
    ///
    /// The default returns [`Default::default()`]; override to construct from the application's
    /// [`crate::core::ProviderRegistry`] (NestJS dependency-injected interceptors).
    fn resolve(_registry: &crate::core::ProviderRegistry) -> Self
    where
        Self: Sized,
    {
        Self::default()
    }

    async fn intercept(&self, req: Request, next: Next) -> Response;
}

/// Logs method, path, status, and elapsed time at **`tracing` `debug`** level (target `nestrs::interceptor`).
#[derive(Default)]
pub struct LoggingInterceptor;

#[async_trait::async_trait]
impl Interceptor for LoggingInterceptor {
    async fn intercept(&self, req: Request, next: Next) -> Response {
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let start = std::time::Instant::now();
        let response = next.run(req).await;
        tracing::debug!(
            target: "nestrs::interceptor",
            method = %method,
            path = %path,
            status = %response.status(),
            elapsed_ms = start.elapsed().as_millis(),
            "request"
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::routing::get;
    use tower::util::ServiceExt;

    #[derive(Default)]
    struct HeaderMarker;

    #[async_trait::async_trait]
    impl Interceptor for HeaderMarker {
        async fn intercept(&self, req: Request, next: Next) -> Response {
            let mut res = next.run(req).await;
            res.headers_mut().insert(
                "x-nestrs-interceptor",
                axum::http::HeaderValue::from_static("ok"),
            );
            res
        }
    }

    #[tokio::test]
    async fn interceptor_layer_runs_before_response() {
        let app = axum::Router::new()
            .route("/", get(|| async { "body" }))
            .layer(crate::interceptor_layer!(HeaderMarker));

        let res = app
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(
            res.headers().get("x-nestrs-interceptor"),
            Some(&axum::http::HeaderValue::from_static("ok"))
        );
    }
}
